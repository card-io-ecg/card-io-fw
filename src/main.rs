#![no_std]
#![no_main]
#![feature(async_fn_in_trait)]
#![feature(type_alias_impl_trait)]
#![feature(let_chains)]
#![feature(never_type)] // Wifi net_task
#![feature(generic_const_exprs)] // norfs needs this
#![allow(incomplete_features)] // generic_const_exprs, async_fn_in_trait

extern crate alloc;

use core::ptr::addr_of;

use alloc::{boxed::Box, rc::Rc};
use embassy_executor::Spawner;
use embassy_sync::{
    blocking_mutex::raw::NoopRawMutex,
    mutex::{Mutex, MutexGuard},
};
use embassy_time::{Duration, Timer};
use norfs::{
    drivers::internal::InternalDriver,
    medium::{cache::ReadCache, StorageMedium},
    Storage, StorageError,
};
use static_cell::{make_static, StaticCell};

#[cfg(feature = "hw_v1")]
use crate::{
    board::{hal::gpio::RTCPinWithResistors, ChargerStatus},
    sleep::disable_gpio_wakeup,
};

#[cfg(feature = "hw_v2")]
use crate::board::VbusDetect;

#[cfg(feature = "battery_max17055")]
use crate::states::battery_info_menu;
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
        initialized::{Board, ConfigPartition},
        startup::StartupResources,
        TouchDetect,
    },
    states::{
        about_menu, adc_setup, app_error, charging, display_menu, initialize, main_menu, measure,
        wifi_ap, wifi_sta, AppMenu,
    },
};

mod board;
mod heap;
mod replace_with;
mod sleep;
mod stack_protection;
mod states;
mod task_control;
mod timeout;

pub type Shared<T> = Rc<Mutex<NoopRawMutex, T>>;
pub type SharedGuard<'a, T> = MutexGuard<'a, NoopRawMutex, T>;

#[derive(Clone, Copy, Debug, PartialEq, defmt::Format)]
pub enum AppError {
    Adc,
}

#[derive(Clone, Copy, Debug, PartialEq, defmt::Format)]
pub enum AppState {
    AdcSetup,
    Initialize,
    Measure,
    Charging,
    Menu(AppMenu),
    Error(AppError),
    Shutdown,
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
    defmt::info!("Hardware version: v1");

    #[cfg(feature = "hw_v2")]
    defmt::info!("Hardware version: v2");

    let executor = make_static!(Executor::new());
    executor.run(move |spawner| {
        spawner.spawn(main_task(spawner, resources)).ok();
    })
}

async fn setup_storage(
) -> Option<&'static mut Storage<&'static mut ReadCache<InternalDriver<ConfigPartition>, 256, 2>>> {
    static mut READ_CACHE: ReadCache<InternalDriver<ConfigPartition>, 256, 2> =
        ReadCache::new(InternalDriver::new(ConfigPartition));

    let storage = match Storage::mount(unsafe { &mut READ_CACHE }).await {
        Ok(storage) => Ok(storage),
        Err(StorageError::NotFormatted) => {
            defmt::info!("Formatting storage");
            Storage::format_and_mount(unsafe { &mut READ_CACHE }).await
        }
        e => e,
    };

    match storage {
        Ok(storage) => Some(make_static!(storage)),
        Err(e) => {
            defmt::error!("Failed to mount storage: {:?}", e);
            None
        }
    }
}

async fn load_config<M: StorageMedium>(storage: Option<&mut Storage<M>>) -> &'static mut Config
where
    [(); M::BLOCK_COUNT]:,
{
    static CONFIG: StaticCell<Config> = StaticCell::new();

    if let Some(storage) = storage {
        defmt::info!(
            "Storage: {} / {} used",
            storage.capacity() - storage.free_bytes(),
            storage.capacity()
        );

        match storage.read("config").await {
            Ok(mut config) => match config.read_loadable::<ConfigFile>(storage).await {
                Ok(config) => CONFIG.init(config.into_config()),
                Err(e) => {
                    defmt::warn!("Failed to read config file: {}. Reverting to defaults", e);
                    CONFIG.init(Config::default())
                }
            },
            Err(e) => {
                defmt::warn!("Failed to load config: {}. Reverting to defaults", e);
                CONFIG.init(Config::default())
            }
        }
    } else {
        defmt::warn!("Storage unavailable. Using default config");
        CONFIG.init(Config::default())
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

    hal::interrupt::enable(
        hal::peripherals::Interrupt::GPIO,
        hal::interrupt::Priority::Priority3,
    )
    .unwrap();

    let mut storage = setup_storage().await;
    let config = load_config(storage.as_deref_mut()).await;
    let mut display = resources.display.enable().await.unwrap();

    let _ = display
        .update_brightness_async(config.display_brightness())
        .await;

    let mut board = Box::pin(async {
        Box::new(Board {
            // If the device is awake, the display should be enabled.
            display,
            frontend: resources.frontend,
            clocks: resources.clocks,
            peripheral_clock_control: resources.peripheral_clock_control,
            high_prio_spawner: INT_EXECUTOR.start(Priority::Priority3),
            battery_monitor,
            wifi: resources.wifi,
            config,
            config_changed: false,
            storage,
        })
    })
    .await;

    let mut state = AppState::AdcSetup;

    loop {
        defmt::info!("New app state: {}", state);
        state = match state {
            AppState::AdcSetup => adc_setup(&mut board).await,
            AppState::Initialize => initialize(&mut board).await,
            AppState::Charging => charging(&mut board).await,
            AppState::Measure => measure(&mut board).await,
            AppState::Menu(AppMenu::Main) => main_menu(&mut board).await,
            AppState::Menu(AppMenu::Display) => display_menu(&mut board).await,
            AppState::Menu(AppMenu::DeviceInfo) => about_menu(&mut board).await,
            AppState::Menu(AppMenu::WifiAP) => wifi_ap(&mut board).await,
            AppState::Menu(AppMenu::WifiListVisible) => wifi_sta(&mut board).await,
            #[cfg(feature = "battery_max17055")]
            AppState::Menu(AppMenu::BatteryInfo) => battery_info_menu(&mut board).await,
            AppState::Error(error) => app_error(&mut board, error).await,
            AppState::Shutdown => break,
        };
    }

    let _ = board.display.shut_down();

    board.frontend.wait_for_release().await;
    Timer::after(Duration::from_millis(100)).await;

    let is_charging = board.battery_monitor.is_plugged();

    #[cfg(feature = "hw_v1")]
    let (_, mut charger_pin) = board.battery_monitor.stop().await;

    #[cfg(feature = "hw_v2")]
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
    wakeup_pins.push((touch, WakeupLevel::Low)).ok().unwrap();

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

        wakeup_pins
            .push((charger_pin, WakeupLevel::Low))
            .ok()
            .unwrap();
    }
}

#[cfg(feature = "hw_v2")]
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

    wakeup_pins.push((touch, WakeupLevel::Low)).ok().unwrap();
    wakeup_pins.push((charger_pin, charger_level)).ok().unwrap();
}
