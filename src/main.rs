#![no_std]
#![no_main]
#![feature(allocator_api)] // Box::try_new
#![feature(type_alias_impl_trait)]
#![feature(let_chains)]
#![feature(never_type)] // Wifi net_task
#![feature(generic_const_exprs)] // norfs needs this
#![feature(impl_trait_in_assoc_type)]
#![allow(incomplete_features)] // generic_const_exprs

extern crate alloc;

#[macro_use]
extern crate logger;

use esp_println as _;

use alloc::{boxed::Box, rc::Rc};
use embassy_executor::Spawner;
use embassy_sync::{
    blocking_mutex::raw::NoopRawMutex,
    mutex::{Mutex, MutexGuard},
};
use embassy_time::{Duration, Timer};
use norfs::{medium::StorageMedium, Storage, StorageError};
use signal_processing::compressing_buffer::CompressingBuffer;
use static_cell::StaticCell;

#[cfg(feature = "battery_max17055")]
pub use crate::states::menu::battery_info::battery_info_menu;

use crate::{
    board::{
        config::{Config, ConfigFile},
        initialized::{Context, InnerContext},
        startup::StartupResources,
        storage::FileSystem,
        TouchDetect,
    },
    states::{
        charging::charging,
        display_serial::display_serial,
        firmware_update::firmware_update,
        init::initialize,
        measure::{measure, ECG_BUFFER_SIZE},
        menu::{
            about::about_menu, display::display_menu, main::main_menu, storage::storage_menu,
            wifi_ap::wifi_ap, wifi_sta::wifi_sta, AppMenu,
        },
        throughput::throughput,
        upload_or_store_measurement::{upload_or_store_measurement, upload_stored_measurements},
        MESSAGE_DURATION,
    },
};

use esp_hal::{
    embassy::executor::{FromCpu1, InterruptExecutor},
    entry,
    interrupt::Priority,
    prelude::{interrupt, main},
    rtc_cntl::{sleep, sleep::WakeupLevel},
    Delay,
};

#[cfg(any(feature = "hw_v4", feature = "hw_v6"))]
use crate::board::VbusDetect;

#[cfg(feature = "esp32s3")]
use esp_hal::gpio::RTCPin as RtcWakeupPin;

#[cfg(feature = "esp32c6")]
use esp_hal::gpio::RTCPinWithResistors as RtcWakeupPin;

mod board;
mod heap;
pub mod human_readable;
mod stack_protection;
mod states;
mod task_control;
mod timeout;

pub struct SerialNumber;

impl SerialNumber {
    pub fn bytes() -> [u8; 6] {
        esp_hal::efuse::Efuse::get_mac_address()
    }
}

impl ufmt::uDisplay for SerialNumber {
    fn fmt<W>(&self, f: &mut ufmt::Formatter<'_, W>) -> Result<(), W::Error>
    where
        W: ufmt::uWrite + ?Sized,
    {
        for byte in Self::bytes() {
            ufmt::uwrite!(f, "{:X}", byte)?;
        }
        Ok(())
    }
}

impl core::fmt::Display for SerialNumber {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let serial = uformat!(12, "{}", self);
        f.write_str(&serial)
    }
}

pub type Shared<T> = Rc<Mutex<NoopRawMutex, T>>;
pub type SharedGuard<'a, T> = MutexGuard<'a, NoopRawMutex, T>;

#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum AppState {
    PreInitialize,
    Initialize,
    Measure,
    Charging,
    Menu(AppMenu),
    DisplaySerial,
    FirmwareUpdate,
    Throughput,
    Shutdown,
    UploadStored(AppMenu),
    UploadOrStore(Box<CompressingBuffer<ECG_BUFFER_SIZE>>),
}

static INT_EXECUTOR: InterruptExecutor<FromCpu1> = InterruptExecutor::new();

#[interrupt]
fn FROM_CPU_INTR1() {
    unsafe { INT_EXECUTOR.on_interrupt() }
}

async fn load_config<M: StorageMedium>(storage: Option<&mut Storage<M>>) -> &'static mut Config
where
    [(); M::BLOCK_COUNT]:,
{
    static CONFIG: StaticCell<Config> = StaticCell::new();

    if let Some(storage) = storage {
        info!(
            "Storage: {} / {} used",
            storage.capacity() - storage.free_bytes(),
            storage.capacity()
        );

        match storage.read("config").await {
            Ok(mut config) => match config.read_loadable::<ConfigFile>(storage).await {
                Ok(config) => return CONFIG.init(config.into_config()),
                Err(e) => {
                    warn!("Failed to read config file: {:?}. Reverting to defaults", e);
                }
            },
            Err(e) => {
                warn!("Failed to load config: {:?}. Reverting to defaults", e);
            }
        }
    } else {
        warn!("Storage unavailable. Using default config");
    }
    CONFIG.init(Config::default())
}

async fn saved_measurement_exists<M>(storage: &mut Storage<M>) -> bool
where
    M: StorageMedium,
    [(); M::BLOCK_COUNT]:,
{
    let mut dir = match storage.read_dir().await {
        Ok(dir) => dir,
        Err(e) => {
            warn!("Failed to open directory: {:?}", e);
            return false;
        }
    };

    let mut buffer = [0; 64];
    loop {
        match dir.next(storage).await {
            Ok(file) => {
                let Some(file) = file else {
                    return false;
                };

                match file.name(storage, &mut buffer).await {
                    Ok(name) => {
                        if name.starts_with("meas.") {
                            return true;
                        }
                    }
                    Err(StorageError::InsufficientBuffer) => {
                        // not a measurement file, ignore
                    }
                    Err(e) => {
                        warn!("Failed to read file name: {:?}", e);
                        return false;
                    }
                }
            }
            Err(e) => {
                warn!("Failed to read directory: {:?}", e);
                return false;
            }
        }
    }
}

#[main]
async fn main(_spawner: Spawner) {
    let resources = StartupResources::initialize().await;

    #[cfg(feature = "hw_v4")]
    info!("Hardware version: v4");

    #[cfg(feature = "hw_v6")]
    info!("Hardware version: v6");

    let mut storage = FileSystem::mount().await;
    let config = load_config(storage.as_deref_mut()).await;

    let mut delay = Delay::new(&resources.clocks);

    // We're boxing Context because we will need to move out of it during shutdown.
    let mut board = Box::new(Context {
        // If the device is awake, the display should be enabled.
        frontend: resources.frontend,
        storage,
        inner: InnerContext {
            display: resources.display,
            clocks: resources.clocks,
            high_prio_spawner: INT_EXECUTOR.start(Priority::Priority3),
            battery_monitor: resources.battery_monitor,
            wifi: resources.wifi,
            config,
            config_changed: true,
            sta_work_available: None,
            message_displayed_at: None,
        },
    });

    unwrap!(board.inner.display.enable().await.ok());

    board.apply_hw_config_changes().await;
    board.config_changed = false;

    let mut state = AppState::PreInitialize;

    loop {
        info!("New app state: {:?}", state);
        state = match state {
            AppState::PreInitialize => {
                if board.battery_monitor.is_plugged() {
                    AppState::Charging
                } else {
                    AppState::Initialize
                }
            }
            AppState::Initialize => initialize(&mut board).await,
            AppState::Charging => charging(&mut board).await,
            AppState::Measure => measure(&mut board).await,
            AppState::Menu(AppMenu::Main) => main_menu(&mut board).await,
            AppState::Menu(AppMenu::Display) => display_menu(&mut board).await,
            AppState::Menu(AppMenu::Storage) => storage_menu(&mut board).await,
            AppState::Menu(AppMenu::DeviceInfo) => about_menu(&mut board).await,
            AppState::Menu(AppMenu::WifiAP) => wifi_ap(&mut board).await,
            AppState::Menu(AppMenu::WifiListVisible) => wifi_sta(&mut board).await,
            #[cfg(feature = "battery_max17055")]
            AppState::Menu(AppMenu::BatteryInfo) => battery_info_menu(&mut board).await,
            AppState::DisplaySerial => display_serial(&mut board).await,
            AppState::FirmwareUpdate => firmware_update(&mut board).await,
            AppState::Throughput => throughput(&mut board).await,
            AppState::UploadStored(next_state) => {
                upload_stored_measurements(&mut board, AppState::Menu(next_state)).await
            }
            AppState::UploadOrStore(buffer) => {
                upload_or_store_measurement(&mut board, buffer, AppState::Shutdown).await
            }
            AppState::Shutdown => break,
        };

        board.wait_for_message(MESSAGE_DURATION).await;
    }

    board.inner.display.shut_down();

    board.frontend.wait_for_release().await;
    Timer::after(Duration::from_millis(100)).await;

    let mut battery_monitor = board.inner.battery_monitor;

    let mut rtc = resources.rtc;
    let is_charging = battery_monitor.is_plugged();

    let (mut charger_pin, _) = battery_monitor.stop().await;

    let (_, _, _, mut touch) = board.frontend.split();

    let mut wakeup_pins = heapless::Vec::<(&mut dyn RtcWakeupPin, WakeupLevel), 2>::new();
    let wakeup_source =
        setup_wakeup_pins(&mut wakeup_pins, &mut touch, &mut charger_pin, is_charging);
    rtc.sleep_deep(&[&wakeup_source], &mut delay);

    // Shouldn't reach this. If we do, we just exit the task, which means the executor
    // will have nothing else to do. Not ideal, but again, we shouldn't reach this.
}

#[cfg(any(feature = "hw_v4", all(feature = "hw_v6", feature = "esp32s3")))]
fn setup_wakeup_pins<'a, 'b, const N: usize>(
    wakeup_pins: &'a mut heapless::Vec<(&'b mut dyn RtcWakeupPin, WakeupLevel), N>,
    touch: &'b mut TouchDetect,
    charger_pin: &'b mut VbusDetect,
    is_charging: bool,
) -> sleep::RtcioWakeupSource<'a, 'b> {
    let charger_level = if is_charging {
        // Wake up momentarily when charger is disconnected
        WakeupLevel::Low
    } else {
        // We want to wake up when the charger is connected, or the electrodes are touched.

        // In v2, the charger status is not connected to an RTC IO pin, so we use the VBUS
        // detect pin instead. This is a high level signal when the charger is connected.
        WakeupLevel::High
    };

    unwrap!(wakeup_pins.push((touch, WakeupLevel::Low)).ok());
    unwrap!(wakeup_pins.push((charger_pin, charger_level)).ok());

    sleep::RtcioWakeupSource::new(wakeup_pins)
}

#[cfg(all(feature = "hw_v6", feature = "esp32c6"))]
fn setup_wakeup_pins<'a, 'b, const N: usize>(
    wakeup_pins: &'a mut heapless::Vec<(&'b mut dyn RtcWakeupPin, WakeupLevel), N>,
    touch: &'b mut TouchDetect,
    charger_pin: &'b mut VbusDetect,
    is_charging: bool,
) -> sleep::Ext1WakeupSource<'a, 'b> {
    let charger_level = if is_charging {
        // Wake up momentarily when charger is disconnected
        WakeupLevel::Low
    } else {
        // We want to wake up when the charger is connected, or the electrodes are touched.

        // In v2, the charger status is not connected to an RTC IO pin, so we use the VBUS
        // detect pin instead. This is a high level signal when the charger is connected.
        WakeupLevel::High
    };

    unwrap!(wakeup_pins.push((touch, WakeupLevel::Low)).ok());
    unwrap!(wakeup_pins.push((charger_pin, charger_level)).ok());

    sleep::Ext1WakeupSource::new(wakeup_pins)
}
