#![no_std]
#![no_main]
#![feature(allocator_api)] // Box::try_new
#![feature(generic_const_exprs)] // norfs needs this
#![feature(impl_trait_in_assoc_type)]
#![allow(incomplete_features)] // generic_const_exprs

extern crate alloc;

// MUST be the first module
mod fmt;

#[cfg(feature = "esp-println")]
use esp_println as _;

use alloc::{boxed::Box, rc::Rc};
use embassy_executor::Spawner;
use embassy_sync::{
    blocking_mutex::raw::NoopRawMutex,
    mutex::{Mutex, MutexGuard},
};
use embassy_time::{Duration, Timer};
#[cfg(feature = "wifi")]
use norfs::StorageError;
use norfs::{medium::StorageMedium, Storage};
use signal_processing::compressing_buffer::CompressingBuffer;
use static_cell::StaticCell;

#[cfg(feature = "wifi")]
use crate::states::{
    firmware_update::firmware_update, throughput::throughput,
    upload_or_store_measurement::upload_stored_measurements,
};
use crate::{
    board::{
        initialized::{Context, InnerContext},
        startup::StartupResources,
        storage::FileSystem,
        TOUCH_PIN, VBUS_DETECT_PIN,
    },
    states::{
        charging::charging,
        display_serial::display_serial,
        init::initialize,
        measure::{measure, ECG_BUFFER_SIZE},
        menu::{display_menu_screen, AppMenu},
        upload_or_store_measurement::upload_or_store_measurement,
        MESSAGE_DURATION,
    },
};
use config_types::{Config, ConfigFile};

use esp_hal::{
    gpio::AnyPin,
    interrupt::Priority,
    rtc_cntl::sleep::{self, WakeupLevel},
};
use esp_rtos::embassy::InterruptExecutor;

esp_bootloader_esp_idf::esp_app_desc!();

#[cfg(feature = "esp32s3")]
use esp_hal::gpio::RtcPin as RtcWakeupPin;

#[cfg(feature = "esp32c6")]
use esp_hal::gpio::RtcPinWithResistors as RtcWakeupPin;

mod board;
pub mod human_readable;
mod states;
mod task_control;
mod timeout;

pub struct SerialNumber;

impl SerialNumber {
    pub fn bytes() -> [u8; 6] {
        esp_hal::efuse::Efuse::mac_address()
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
    #[cfg(feature = "wifi")]
    FirmwareUpdate,
    #[cfg(feature = "wifi")]
    Throughput,
    Shutdown,
    #[cfg(feature = "wifi")]
    UploadStored(AppMenu),
    UploadOrStore(Box<CompressingBuffer<ECG_BUFFER_SIZE>>),
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

#[cfg(feature = "wifi")]
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

#[esp_rtos::main]
async fn main(_spawner: Spawner) {
    #[cfg(all(feature = "rtt", feature = "defmt"))]
    rtt_target::rtt_init_defmt!();

    esp_alloc::heap_allocator!(size: (48 + 96) * 1024);

    let resources = StartupResources::initialize().await;

    static INTERRUPT_EXECUTOR: StaticCell<InterruptExecutor<2>> = StaticCell::new();
    let interrupt_executor =
        INTERRUPT_EXECUTOR.init(InterruptExecutor::new(resources.software_interrupt2));

    info!("Hardware version: {}", env!("HW_VERSION"));

    let mut storage = FileSystem::mount().await;
    let config = load_config(storage.as_deref_mut()).await;

    // We're boxing Context because we will need to move out of it during shutdown.
    let mut board = Box::new(Context {
        // If the device is awake, the display should be enabled.
        frontend: resources.frontend,
        storage,
        inner: InnerContext {
            display: resources.display,
            high_prio_spawner: interrupt_executor.start(Priority::Priority2),
            battery_monitor: resources.battery_monitor,
            #[cfg(feature = "wifi")]
            wifi: {
                use board::wifi::WifiDriver;
                static WIFI: StaticCell<WifiDriver> = StaticCell::new();
                WIFI.init(WifiDriver::new(resources.wifi))
            },
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
            AppState::Menu(menu) => display_menu_screen(menu, &mut board).await,
            AppState::DisplaySerial => display_serial(&mut board).await,
            #[cfg(feature = "wifi")]
            AppState::FirmwareUpdate => firmware_update(&mut board).await,
            #[cfg(feature = "wifi")]
            AppState::Throughput => throughput(&mut board).await,
            #[cfg(feature = "wifi")]
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

    let is_charging = battery_monitor.is_plugged();

    let (_charger_pin, _) = battery_monitor.stop().await;
    let (_, _, _, _touch) = board.frontend.split();

    enter_sleep(resources.rtc, is_charging);
    // Shouldn't reach this. If we do, we just exit the task, which means the executor
    // will have nothing else to do. Not ideal, but again, we shouldn't reach this.
}

fn enter_sleep(mut rtc: esp_hal::rtc_cntl::Rtc, is_charging: bool) {
    let charger_level = if is_charging {
        // Wake up momentarily when charger is disconnected
        WakeupLevel::Low
    } else {
        // We want to wake up when the charger is connected, or the electrodes are touched.

        // In v2, the charger status is not connected to an RTC IO pin, so we use the VBUS
        // detect pin instead. This is a high level signal when the charger is connected.
        WakeupLevel::High
    };

    let mut touch = unsafe { AnyPin::steal(TOUCH_PIN) };
    let mut charger_pin = unsafe { AnyPin::steal(VBUS_DETECT_PIN) };

    let mut wakeup_pins: [(&mut dyn RtcWakeupPin, WakeupLevel); 2] = [
        (&mut touch, WakeupLevel::Low),
        (&mut charger_pin, charger_level),
    ];

    #[cfg(feature = "esp32s3")]
    let wakeup_source = sleep::RtcioWakeupSource::new(&mut wakeup_pins);

    #[cfg(not(feature = "esp32s3"))]
    let wakeup_source = sleep::Ext1WakeupSource::new(&mut wakeup_pins);

    rtc.sleep_deep(&[&wakeup_source]);
}
