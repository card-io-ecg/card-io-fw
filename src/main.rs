#![no_std]
#![no_main]
#![feature(allocator_api)] // Box::try_new
#![feature(async_fn_in_trait)]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_projections)]
#![feature(let_chains)]
#![feature(never_type)] // Wifi net_task
#![feature(generic_const_exprs)] // norfs needs this
#![feature(return_position_impl_trait_in_trait)]
#![feature(impl_trait_in_assoc_type)]
#![allow(incomplete_features)] // generic_const_exprs, async_fn_in_trait

extern crate alloc;

#[macro_use]
extern crate logger;

use core::ptr::addr_of;

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
use static_cell::{make_static, StaticCell};

#[cfg(feature = "hw_v1")]
use crate::{
    board::{hal::gpio::RTCPinWithResistors, ChargerStatus},
    sleep::disable_gpio_wakeup,
};

#[cfg(any(feature = "hw_v2", feature = "hw_v4"))]
use crate::board::VbusDetect;

#[cfg(feature = "battery_max17055")]
pub use crate::states::menu::battery_info::battery_info_menu;

use crate::{
    board::{
        config::{Config, ConfigFile},
        drivers::battery_monitor::BatteryMonitor,
        hal::{
            self,
            embassy::executor::{Executor, FromCpu1, InterruptExecutor},
            entry,
            gpio::RTCPin,
            interrupt::Priority,
            prelude::interrupt,
            rtc_cntl::sleep::{RtcioWakeupSource, WakeupLevel},
            Delay,
        },
        initialized::Board,
        startup::StartupResources,
        storage::FileSystem,
        TouchDetect,
    },
    states::{
        adc_setup::adc_setup,
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

mod board;
mod heap;
pub mod human_readable;
mod replace_with;
mod sleep;
mod stack_protection;
mod states;
mod task_control;
mod timeout;

pub struct SerialNumber;

impl SerialNumber {
    pub fn bytes() -> [u8; 6] {
        hal::efuse::Efuse::get_mac_address()
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

pub enum AppState {
    AdcSetup,
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

impl core::fmt::Debug for AppState {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::AdcSetup => write!(f, "AdcSetup"),
            Self::Initialize => write!(f, "Initialize"),
            Self::Measure => write!(f, "Measure"),
            Self::Charging => write!(f, "Charging"),
            Self::Menu(arg0) => f.debug_tuple("Menu").field(arg0).finish(),
            Self::DisplaySerial => write!(f, "DisplaySerial"),
            Self::FirmwareUpdate => write!(f, "FirmwareUpdate"),
            Self::Throughput => write!(f, "Throughput"),
            Self::Shutdown => write!(f, "Shutdown"),
            Self::UploadStored(arg0) => f.debug_tuple("UploadStored").field(arg0).finish(),
            Self::UploadOrStore(buf) => f.debug_tuple("UploadOrStore").field(&buf.len()).finish(),
        }
    }
}

#[cfg(feature = "defmt")]
impl defmt::Format for AppState {
    fn format(&self, f: defmt::Formatter) {
        match self {
            Self::AdcSetup => defmt::write!(f, "AdcSetup"),
            Self::Initialize => defmt::write!(f, "Initialize"),
            Self::Measure => defmt::write!(f, "Measure"),
            Self::Charging => defmt::write!(f, "Charging"),
            Self::Menu(arg0) => defmt::write!(f, "Menu({:?})", arg0),
            Self::DisplaySerial => defmt::write!(f, "DisplaySerial"),
            Self::FirmwareUpdate => defmt::write!(f, "FirmwareUpdate"),
            Self::Throughput => defmt::write!(f, "Throughput"),
            Self::Shutdown => defmt::write!(f, "Shutdown"),
            Self::UploadStored(arg0) => defmt::write!(f, "UploadStored({:?})", arg0),
            Self::UploadOrStore(buf) => defmt::write!(f, "UploadOrStore (len={})", buf.len()),
        }
    }
}

static INT_EXECUTOR: InterruptExecutor<FromCpu1> = InterruptExecutor::new();

#[interrupt]
fn FROM_CPU_INTR1() {
    unsafe { INT_EXECUTOR.on_interrupt() }
}

extern "C" {
    static mut _stack_start_cpu0: u8;
    static mut _stack_end_cpu0: u8;
}

#[entry]
fn main() -> ! {
    // Board::initialize initialized embassy so it must be called first.
    let resources = StartupResources::initialize();

    // We only use a single core for now, so we can write both stack regions.
    let stack_start = unsafe { addr_of!(_stack_start_cpu0) as usize };
    let stack_end = unsafe { addr_of!(_stack_end_cpu0) as usize };
    let _stack_protection = stack_protection::StackMonitor::protect((stack_start + 4)..stack_end);

    #[cfg(feature = "hw_v1")]
    info!("Hardware version: v1");

    #[cfg(feature = "hw_v2")]
    info!("Hardware version: v2");

    let executor = make_static!(Executor::new());
    executor.run(move |spawner| {
        spawner.spawn(main_task(spawner, resources)).ok();
    })
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
                Ok(config) => CONFIG.init(config.into_config()),
                Err(e) => {
                    warn!("Failed to read config file: {:?}. Reverting to defaults", e);
                    CONFIG.init(Config::default())
                }
            },
            Err(e) => {
                warn!("Failed to load config: {:?}. Reverting to defaults", e);
                CONFIG.init(Config::default())
            }
        }
    } else {
        warn!("Storage unavailable. Using default config");
        CONFIG.init(Config::default())
    }
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

#[embassy_executor::task]
async fn main_task(_spawner: Spawner, resources: StartupResources) {
    let battery_monitor = BatteryMonitor::start(
        resources.misc_pins.vbus_detect,
        resources.misc_pins.chg_status,
        #[cfg(feature = "battery_adc")]
        resources.battery_adc,
        #[cfg(feature = "battery_max17055")]
        resources.battery_fg,
    )
    .await;

    unwrap!(hal::interrupt::enable(
        hal::peripherals::Interrupt::GPIO,
        hal::interrupt::Priority::Priority3,
    ));

    let mut storage = FileSystem::mount().await;
    let config = load_config(storage.as_deref_mut()).await;
    let mut display = unwrap!(resources.display.enable().await.ok());

    let _ = display
        .update_brightness_async(config.display_brightness())
        .await;

    let mut board = Box::pin(async {
        Box::new(Board {
            // If the device is awake, the display should be enabled.
            display,
            frontend: resources.frontend,
            clocks: resources.clocks,
            high_prio_spawner: INT_EXECUTOR.start(Priority::Priority3),
            battery_monitor,
            wifi: resources.wifi,
            config,
            config_changed: false,
            storage,
            sta_work_available: None,
            message_displayed_at: None,
        })
    })
    .await;

    let mut state = AppState::AdcSetup;

    loop {
        info!("New app state: {:?}", state);
        state = match state {
            AppState::AdcSetup => adc_setup(&mut board).await,
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

        if let Some(message_at) = board.message_displayed_at.take() {
            Timer::at(message_at + MESSAGE_DURATION).await;
        }
    }

    let _ = board.display.shut_down();

    board.frontend.wait_for_release().await;
    Timer::after(Duration::from_millis(100)).await;

    let is_charging = board.battery_monitor.is_plugged();

    #[cfg(feature = "hw_v1")]
    let (_, mut charger_pin) = board.battery_monitor.stop().await;

    #[cfg(any(feature = "hw_v2", feature = "hw_v4"))]
    let (mut charger_pin, _) = board.battery_monitor.stop().await;

    let (_, _, _, mut touch) = board.frontend.split();
    let mut rtc = resources.rtc;

    let mut wakeup_pins = heapless::Vec::<(&mut dyn RTCPin, WakeupLevel), 2>::new();
    setup_wakeup_pins(&mut wakeup_pins, &mut touch, &mut charger_pin, is_charging);
    let rtcio = RtcioWakeupSource::new(&mut wakeup_pins);

    rtc.sleep_deep(&[&rtcio], &mut Delay::new(&board.clocks));

    // Shouldn't reach this. If we do, we just exit the task, which means the executor
    // will have nothing else to do. Not ideal, but again, we shouldn't reach this.
}

#[cfg(feature = "hw_v1")]
fn setup_wakeup_pins<'a, const N: usize>(
    wakeup_pins: &mut heapless::Vec<(&'a mut dyn RTCPin, WakeupLevel), N>,
    touch: &'a mut TouchDetect,
    charger_pin: &'a mut ChargerStatus,
    is_charging: bool,
) {
    unwrap!(wakeup_pins.push((touch, WakeupLevel::Low)).ok());

    if is_charging {
        // This is a bit awkward as unplugging then replugging will not wake the
        // device. Ideally, we'd use the VBUS detect pin, but it's not connected to RTCIO.
        disable_gpio_wakeup(charger_pin);
    } else {
        // We want to wake up when the charger is connected, or the electrodes are touched.

        // v1 uses the charger status pin, which is open drain
        // and the board does not have a pullup resistor. A low signal means the battery is
        // charging. This means we can watch for low level to detect a charger connection.
        charger_pin.rtcio_pad_hold(true);
        charger_pin.rtcio_pullup(true);

        unwrap!(wakeup_pins.push((charger_pin, WakeupLevel::Low)).ok());
    }
}

#[cfg(any(feature = "hw_v2", feature = "hw_v4"))]
fn setup_wakeup_pins<'a, const N: usize>(
    wakeup_pins: &mut heapless::Vec<(&'a mut dyn RTCPin, WakeupLevel), N>,
    touch: &'a mut TouchDetect,
    charger_pin: &'a mut VbusDetect,
    is_charging: bool,
) {
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
}
