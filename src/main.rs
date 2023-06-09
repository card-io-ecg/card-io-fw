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

use embassy_executor::{Executor, Spawner, _export::StaticCell};
use embassy_sync::{blocking_mutex::raw::NoopRawMutex, mutex::Mutex, signal::Signal};
use embassy_time::{Duration, Timer};
use norfs::{
    drivers::internal::InternalDriver,
    medium::{cache::ReadCache, StorageMedium},
    Storage, StorageError,
};

#[cfg(feature = "battery_adc")]
use crate::board::drivers::battery_adc::monitor_task_adc;

#[cfg(feature = "battery_max17055")]
use crate::board::drivers::battery_fg::monitor_task_fg;

#[cfg(feature = "hw_v1")]
use crate::sleep::{disable_gpio_wakeup, enable_gpio_pullup};
use crate::{
    board::{
        config::{Config, ConfigFile},
        hal::{self, entry, prelude::interrupt},
        initialized::{BatteryMonitor, BatteryState, Board, ConfigPartition},
        startup::StartupResources,
        BATTERY_MODEL,
    },
    interrupt::{InterruptExecutor, SwInterrupt0},
    sleep::{enable_gpio_wakeup, start_deep_sleep, RtcioWakeupType},
    states::{
        adc_setup, app_error, charging, display_menu, initialize, main_menu, measure, wifi_ap,
    },
};

mod board;
mod heap;
mod interrupt;
mod replace_with;
mod sleep;
mod stack_protection;
mod states;
mod task_control;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AppError {
    Adc,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AppState {
    AdcSetup,
    Initialize,
    Measure,
    Charging,
    MainMenu,
    DisplayMenu,
    WifiAP,
    Error(AppError),
    Shutdown,
}

pub type SharedBatteryState = Mutex<NoopRawMutex, BatteryState>;

static INT_EXECUTOR: InterruptExecutor<SwInterrupt0> = InterruptExecutor::new();

macro_rules! singleton {
    ($val:expr) => {{
        type T = impl Sized;
        static STATIC_CELL: StaticCell<T> = StaticCell::new();
        let (x,) = STATIC_CELL.init(($val,));
        x
    }};
}

#[interrupt]
fn FROM_CPU_INTR0() {
    unsafe { INT_EXECUTOR.on_interrupt() }
}

extern "C" {
    static mut _stack_start_cpu0: u8;
    static mut _stack_end_cpu0: u8;

    static mut _stack_start_cpu1: u8;
    static mut _stack_end_cpu1: u8;
}

#[entry]
fn main() -> ! {
    // Board::initialize initialized embassy so it must be called first.
    let resources = StartupResources::initialize();

    // We only use a single core for now, so we can write both stack regions.
    let stack_start = unsafe { addr_of!(_stack_start_cpu1) as u32 };
    let stack_end = unsafe { addr_of!(_stack_end_cpu0) as u32 };
    let _stack_protection = stack_protection::StackMonitor::protect((stack_start + 4)..stack_end);

    #[cfg(feature = "hw_v1")]
    log::info!("Hardware version: v1");

    #[cfg(feature = "hw_v2")]
    log::info!("Hardware version: v2");

    let executor = singleton!(Executor::new());
    executor.run(move |spawner| {
        spawner.spawn(main_task(spawner, resources)).ok();
    });
}

async fn setup_storage(
) -> Option<&'static mut Storage<&'static mut ReadCache<InternalDriver<ConfigPartition>, 256, 2>>> {
    static mut READ_CACHE: ReadCache<InternalDriver<ConfigPartition>, 256, 2> =
        ReadCache::new(InternalDriver::new(ConfigPartition));

    let storage = match Storage::mount(unsafe { &mut READ_CACHE }).await {
        Ok(storage) => Ok(storage),
        Err(StorageError::NotFormatted) => {
            log::info!("Formatting storage");
            Storage::format_and_mount(unsafe { &mut READ_CACHE }).await
        }
        e => e,
    };

    match storage {
        Ok(storage) => Some(singleton!(storage)),
        Err(e) => {
            log::error!("Failed to mount storage: {:?}", e);
            None
        }
    }
}

async fn load_config<M: StorageMedium>(storage: &mut Option<&mut Storage<M>>) -> &'static mut Config
where
    [(); M::BLOCK_COUNT]:,
{
    &mut *singleton!(if let Some(storage) = storage {
        log::info!(
            "Storage: {} / {} used",
            storage.capacity() - storage.free_bytes(),
            storage.capacity()
        );

        match storage.read("config").await {
            Ok(mut config) => match config.read_loadable::<ConfigFile>(storage).await {
                Ok(config) => config.into_config(),
                Err(e) => {
                    log::warn!("Failed to read config file: {e:?}. Reverting to defaults");
                    Config::default()
                }
            },
            Err(e) => {
                log::warn!("Failed to load config: {e:?}. Reverting to defaults");
                Config::default()
            }
        }
    } else {
        log::warn!("Storage unavailable. Using default config");
        Config::default()
    })
}

#[embassy_executor::task]
async fn main_task(spawner: Spawner, resources: StartupResources) {
    #[cfg(any(feature = "battery_adc", feature = "battery_max17055"))]
    let fg_task_control = &*singleton!(Signal::new());

    let battery_state = &*singleton!(Mutex::new(BatteryState {
        #[cfg(feature = "battery_adc")]
        adc_data: None,
        #[cfg(feature = "battery_max17055")]
        fg_data: None,
    }));

    #[cfg(feature = "battery_adc")]
    spawner
        .spawn(monitor_task_adc(
            resources.battery_adc,
            battery_state,
            fg_task_control,
        ))
        .ok();

    #[cfg(feature = "battery_max17055")]
    spawner
        .spawn(monitor_task_fg(
            resources.battery_fg,
            battery_state,
            fg_task_control,
        ))
        .ok();

    hal::interrupt::enable(
        hal::peripherals::Interrupt::GPIO,
        hal::interrupt::Priority::Priority3,
    )
    .unwrap();

    let mut storage = setup_storage().await;
    let config = load_config(&mut storage).await;

    let mut board = Board {
        // If the device is awake, the display should be enabled.
        display: resources.display.enable().await.unwrap(),
        frontend: resources.frontend,
        clocks: resources.clocks,
        peripheral_clock_control: resources.peripheral_clock_control,
        high_prio_spawner: INT_EXECUTOR.start(),
        battery_monitor: BatteryMonitor {
            model: BATTERY_MODEL,
            battery_state,
            vbus_detect: resources.misc_pins.vbus_detect,
            charger_status: resources.misc_pins.chg_status,
        },
        wifi: resources.wifi,
        config,
        config_changed: false,
        storage,
    };

    let _ = board
        .display
        .update_brightness_async(board.config.display_brightness())
        .await;

    let mut state = AppState::AdcSetup;

    loop {
        log::info!("New app state: {state:?}");
        state = match state {
            AppState::AdcSetup => adc_setup(&mut board).await,
            AppState::Initialize => initialize(&mut board).await,
            AppState::Charging => charging(&mut board).await,
            AppState::Measure => measure(&mut board).await,
            AppState::MainMenu => main_menu(&mut board).await,
            AppState::DisplayMenu => display_menu(&mut board).await,
            AppState::WifiAP => wifi_ap(&mut board).await,
            AppState::Error(error) => app_error(&mut board, error).await,
            AppState::Shutdown => break,
        };
    }

    let _ = board.display.shut_down();

    #[cfg(any(feature = "battery_adc", feature = "battery_max17055"))]
    fg_task_control.signal(());

    board.frontend.wait_for_release().await;
    Timer::after(Duration::from_millis(100)).await;

    let is_charging = board.battery_monitor.is_plugged();
    let (_, _, _, touch) = board.frontend.split();

    #[cfg(feature = "hw_v1")]
    let charger_pin = board.battery_monitor.charger_status;

    #[cfg(feature = "hw_v2")]
    let charger_pin = board.battery_monitor.vbus_detect;

    enable_gpio_wakeup(&touch, RtcioWakeupType::LowLevel);

    if is_charging {
        #[cfg(feature = "hw_v1")]
        {
            // This is a bit awkward as unplugging then replugging will not wake the
            // device. Ideally, we'd use the VBUS detect pin, but it's not connected to RTCIO.
            disable_gpio_wakeup(&charger_pin);
        }

        // Wake up momentarily when charger is disconnected
        #[cfg(feature = "hw_v2")]
        enable_gpio_wakeup(&charger_pin, RtcioWakeupType::LowLevel);
    } else {
        // We want to wake up when the charger is connected, or the electrodes are touched.

        // v1 uses the charger status pin, which is open drain
        // and the board does not have a pullup resistor. A low signal means the battery is
        // charging. This means we can watch for low level to detect a charger connection.
        #[cfg(feature = "hw_v1")]
        {
            enable_gpio_pullup(&charger_pin);
            enable_gpio_wakeup(&charger_pin, RtcioWakeupType::LowLevel);
        }

        // In v2, the charger status is not connected to an RTC IO pin, so we use the VBUS
        // detect pin instead. This is a high level signal when the charger is connected.
        #[cfg(feature = "hw_v2")]
        enable_gpio_wakeup(&charger_pin, RtcioWakeupType::HighLevel);
    }

    start_deep_sleep();

    // Shouldn't reach this. If we do, we just exit the task, which means the executor
    // will have nothing else to do. Not ideal, but again, we shouldn't reach this.
}
