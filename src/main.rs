#![no_std]
#![no_main]
#![feature(async_fn_in_trait)]
#![feature(type_alias_impl_trait)]
#![feature(let_chains)]
#![allow(incomplete_features)]

extern crate alloc;

use embassy_executor::{Executor, Spawner, _export::StaticCell};
use embassy_sync::{blocking_mutex::raw::NoopRawMutex, mutex::Mutex, signal::Signal};
use embassy_time::{Duration, Ticker};
use embedded_hal::digital::OutputPin;

use crate::{
    board::{
        hal::{self, entry},
        initialized::{BatteryMonitor, Board},
        startup::StartupResources,
        BatteryAdc, Config,
    },
    sleep::enter_deep_sleep,
    states::{adc_setup, app_error, charging, display_menu, initialize, main_menu, measure},
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
    Error(AppError),
    Shutdown,
}

pub struct BatteryState {
    pub charging_current: Option<u16>,
    pub battery_voltage: Option<u16>,
}

pub type SharedBatteryState = Mutex<NoopRawMutex, BatteryState>;

static EXECUTOR: StaticCell<Executor> = StaticCell::new();
static BATTERY_STATE: StaticCell<SharedBatteryState> = StaticCell::new();
static TASK_CONTROL: StaticCell<Signal<NoopRawMutex, ()>> = StaticCell::new();

#[entry]
fn main() -> ! {
    // Board::initialize initialized embassy so it must be called first.
    let resources = StartupResources::initialize();

    let executor = EXECUTOR.init(Executor::new());
    executor.run(move |spawner| {
        spawner.spawn(main_task(spawner, resources)).ok();
    });
}

#[embassy_executor::task]
async fn main_task(spawner: Spawner, resources: StartupResources) {
    let task_control = &*TASK_CONTROL.init_with(Signal::new);

    let battery_state = BATTERY_STATE.init(Mutex::new(BatteryState {
        charging_current: None,
        battery_voltage: None,
    }));

    spawner
        .spawn(monitor_task(
            resources.battery_adc,
            battery_state,
            task_control.clone(),
        ))
        .ok();

    hal::interrupt::enable(
        hal::peripherals::Interrupt::GPIO,
        hal::interrupt::Priority::Priority3,
    )
    .unwrap();

    let mut board = Board {
        // If the device is awake, the display should be enabled.
        display: resources.display.enable().await.unwrap(),
        frontend: resources.frontend,
        clocks: resources.clocks,
        high_prio_spawner: resources.high_prio_spawner,
        battery_monitor: BatteryMonitor {
            battery_state,
            vbus_detect: resources.misc_pins.vbus_detect,
            charger_status: resources.misc_pins.chg_status,
        },
        config: Config::default(), // TODO: load config
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
            AppState::Error(error) => app_error(&mut board, error).await,
            AppState::Shutdown => {
                let _ = board.display.shut_down();

                task_control.signal(());

                let (_, _, _, touch) = board.frontend.split();
                let charger_pin = board.battery_monitor.charger_status;

                enter_deep_sleep(touch, charger_pin).await
            }
        };
    }
}

// Debug task, to be removed
#[embassy_executor::task]
async fn monitor_task(
    mut battery: BatteryAdc,
    battery_state: &'static SharedBatteryState,
    task_control: &'static Signal<NoopRawMutex, ()>,
) {
    let mut timer = Ticker::every(Duration::from_millis(10));

    battery.enable.set_high().unwrap();

    let mut voltage_accumulator = 0;
    let mut current_accumulator = 0;

    let mut sample_count = 0;

    const AVG_SAMPLE_COUNT: u32 = 128;

    while !task_control.signaled() {
        let voltage = battery.read_battery_voltage().await;
        let current = battery.read_charge_current().await;

        voltage_accumulator += voltage.unwrap() as u32;
        current_accumulator += current.unwrap() as u32;

        if sample_count == AVG_SAMPLE_COUNT {
            let voltage = (voltage_accumulator / AVG_SAMPLE_COUNT) as u16;
            let current = (current_accumulator / AVG_SAMPLE_COUNT) as u16;

            let mut state = battery_state.lock().await;
            state.battery_voltage = Some(voltage);
            state.charging_current = Some(current);

            log::debug!("Voltage = {voltage:?}");
            log::debug!("Current = {current:?}");

            sample_count = 0;

            voltage_accumulator = 0;
            current_accumulator = 0;
        } else {
            sample_count += 1;
        }

        timer.next().await;
    }

    log::debug!("Monitor exited");
}
