#![no_std]
#![no_main]
#![feature(async_fn_in_trait)]
#![feature(type_alias_impl_trait)]
#![feature(let_chains)]
#![feature(generic_const_exprs)] // norfs needs this
#![allow(incomplete_features)] // generic_const_exprs, async_fn_in_trait

extern crate alloc;

use embassy_executor::{Executor, Spawner, _export::StaticCell};
use embassy_sync::{blocking_mutex::raw::NoopRawMutex, mutex::Mutex, signal::Signal};
use embassy_time::{Duration, Ticker, Timer};
#[cfg(any(feature = "hw_v1", feature = "battery_adc"))]
use embedded_hal::digital::OutputPin;
use embedded_hal_async::digital::Wait;
use norfs::{drivers::internal::InternalDriver, medium::cache::ReadCache, Storage, StorageError};

#[cfg(feature = "battery_adc")]
use crate::board::{drivers::battery_adc::BatteryAdcData, BatteryAdc};

#[cfg(feature = "battery_max17055")]
use crate::board::BatteryFg;

#[cfg(feature = "hw_v1")]
use crate::sleep::enable_gpio_pullup;
use crate::{
    board::{
        config::{Config, ConfigFile},
        hal::{self, entry},
        initialized::{BatteryMonitor, BatteryState, Board, ConfigPartition},
        startup::StartupResources,
        BATTERY_MODEL,
    },
    sleep::{disable_gpio_wakeup, enable_gpio_wakeup, start_deep_sleep, RtcioWakeupType},
    states::{
        adc_setup, app_error, charging, display_menu, initialize, main_menu, measure, wifi_ap,
    },
};

mod board;
mod heap;
mod interrupt;
mod replace_with;
mod sleep;
mod states;

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
    ShutdownCharging,
}

pub type SharedBatteryState = Mutex<NoopRawMutex, BatteryState>;

static EXECUTOR: StaticCell<Executor> = StaticCell::new();
static BATTERY_STATE: StaticCell<SharedBatteryState> = StaticCell::new();
#[cfg(feature = "battery_adc")]
static ADC_TASK_CONTROL: StaticCell<Signal<NoopRawMutex, ()>> = StaticCell::new();
#[cfg(feature = "battery_max17055")]
static FG_TASK_CONTROL: StaticCell<Signal<NoopRawMutex, ()>> = StaticCell::new();

#[entry]
fn main() -> ! {
    // Board::initialize initialized embassy so it must be called first.
    let resources = StartupResources::initialize();

    #[cfg(feature = "hw_v1")]
    log::info!("Hardware version: v1");

    #[cfg(feature = "hw_v2")]
    log::info!("Hardware version: v2");

    let executor = EXECUTOR.init(Executor::new());
    executor.run(move |spawner| {
        spawner.spawn(main_task(spawner, resources)).ok();
    });
}

#[embassy_executor::task]
async fn main_task(spawner: Spawner, resources: StartupResources) {
    #[cfg(feature = "battery_adc")]
    let adc_task_control = &*ADC_TASK_CONTROL.init_with(Signal::new);

    #[cfg(feature = "battery_max17055")]
    let fg_task_control = &*FG_TASK_CONTROL.init_with(Signal::new);

    let battery_state = BATTERY_STATE.init(Mutex::new(BatteryState {
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
            adc_task_control,
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

    let storage = match Storage::mount(ReadCache::<_, 256, 2>::new(InternalDriver::new(
        ConfigPartition,
    )))
    .await
    {
        Ok(storage) => Ok(storage),
        Err(StorageError::NotFormatted) => {
            log::info!("Formatting storage");
            Storage::format_and_mount(ReadCache::new(InternalDriver::new(ConfigPartition))).await
        }
        e => e,
    };

    let mut storage = match storage {
        Ok(storage) => Some(storage),
        Err(e) => {
            log::error!("Failed to mount storage: {:?}", e);
            None
        }
    };

    let config = if let Some(storage) = storage.as_mut() {
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
    };

    let mut board = Board {
        // If the device is awake, the display should be enabled.
        display: resources.display.enable().await.unwrap(),
        frontend: resources.frontend,
        clocks: resources.clocks,
        peripheral_clock_control: resources.peripheral_clock_control,
        high_prio_spawner: resources.high_prio_spawner,
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
            AppState::Shutdown => {
                let _ = board.display.shut_down();

                #[cfg(feature = "battery_adc")]
                adc_task_control.signal(());
                #[cfg(feature = "battery_max17055")]
                fg_task_control.signal(());

                let (_, _, _, mut touch) = board.frontend.split();

                #[cfg(feature = "hw_v1")]
                let charger_pin = board.battery_monitor.charger_status;

                #[cfg(feature = "hw_v2")]
                let charger_pin = board.battery_monitor.vbus_detect;

                touch.wait_for_high().await.unwrap();
                Timer::after(Duration::from_millis(100)).await;

                #[cfg(feature = "hw_v1")]
                enable_gpio_pullup(&charger_pin);

                enable_gpio_wakeup(&touch, RtcioWakeupType::LowLevel);
                enable_gpio_wakeup(&charger_pin, RtcioWakeupType::LowLevel);

                // Wake up momentarily when charger is disconnected
                start_deep_sleep();

                // Shouldn't reach this. If we do, we just exit the task, which means the executor
                // will have nothing else to do. Not ideal, but again, we shouldn't reach this.
                return;
            }
            AppState::ShutdownCharging => {
                let _ = board.display.shut_down();

                #[cfg(feature = "battery_adc")]
                adc_task_control.signal(());
                #[cfg(feature = "battery_max17055")]
                fg_task_control.signal(());

                let (_, _, _, mut touch) = board.frontend.split();

                #[cfg(feature = "hw_v1")]
                let charger_pin = board.battery_monitor.charger_status;

                #[cfg(feature = "hw_v2")]
                let charger_pin = board.battery_monitor.vbus_detect;

                touch.wait_for_high().await.unwrap();
                Timer::after(Duration::from_millis(100)).await;

                enable_gpio_wakeup(&touch, RtcioWakeupType::LowLevel);

                // FIXME: This is a bit awkward as unplugging then replugging will not wake the
                // device. Ideally, we'd use the VBUS detect pin, but it's not connected to RTCIO.
                disable_gpio_wakeup(&charger_pin);

                // Wake up momentarily when charger is disconnected
                start_deep_sleep();

                // Shouldn't reach this. If we do, we just exit the task, which means the executor
                // will have nothing else to do. Not ideal, but again, we shouldn't reach this.
                return;
            }
        };
    }
}

#[cfg(feature = "battery_adc")]
#[embassy_executor::task]
async fn monitor_task_adc(
    mut battery: BatteryAdc,
    battery_state: &'static SharedBatteryState,
    task_control: &'static Signal<NoopRawMutex, ()>,
) {
    let mut timer = Ticker::every(Duration::from_millis(10));
    log::debug!("ADC monitor started");

    battery.enable.set_high().unwrap();

    let mut voltage_accumulator = 0;
    let mut current_accumulator = 0;

    let mut sample_count = 0;

    const AVG_SAMPLE_COUNT: u32 = 128;

    while !task_control.signaled() {
        let data = battery.read_data().await.unwrap();

        voltage_accumulator += data.voltage as u32;
        current_accumulator += data.charge_current as u32;

        if sample_count == AVG_SAMPLE_COUNT {
            let mut state = battery_state.lock().await;

            let average = BatteryAdcData {
                voltage: (voltage_accumulator / AVG_SAMPLE_COUNT) as u16,
                charge_current: (current_accumulator / AVG_SAMPLE_COUNT) as u16,
            };
            state.adc_data = Some(average);

            log::debug!("Battery data: {average:?}");

            sample_count = 0;

            voltage_accumulator = 0;
            current_accumulator = 0;
        } else {
            sample_count += 1;
        }

        timer.next().await;
    }

    battery.enable.set_low().unwrap();

    log::debug!("Monitor exited");
}

#[cfg(feature = "battery_max17055")]
#[embassy_executor::task]
async fn monitor_task_fg(
    mut fuel_gauge: BatteryFg,
    battery_state: &'static SharedBatteryState,
    task_control: &'static Signal<NoopRawMutex, ()>,
) {
    use embassy_time::Delay;

    let mut timer = Ticker::every(Duration::from_secs(1));
    log::debug!("Fuel gauge monitor started");

    fuel_gauge.enable(&mut Delay).await;

    while !task_control.signaled() {
        let data = fuel_gauge.read_data().await.unwrap();

        {
            let mut state = battery_state.lock().await;
            state.fg_data = Some(data);
        }
        log::debug!("Battery data: {data:?}");

        timer.next().await;
    }

    fuel_gauge.disable();

    log::debug!("Monitor exited");
}
