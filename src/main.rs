#![no_std]
#![no_main]
#![feature(async_fn_in_trait)]
#![feature(type_alias_impl_trait)]
#![feature(let_chains)]
#![allow(incomplete_features)]

extern crate alloc;

use embassy_executor::{Executor, Spawner, _export::StaticCell};
use embassy_sync::{blocking_mutex::raw::NoopRawMutex, mutex::Mutex};
use embassy_time::{Duration, Ticker};
use embedded_hal::digital::OutputPin;

use crate::{
    board::{
        hal::{self, entry},
        initialized::{BatteryMonitor, Board},
        startup::StartupResources,
        BatteryAdc,
    },
    sleep::enter_deep_sleep,
    states::{adc_setup, app_error, display_menu, initialize, main_menu, measure},
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
    let battery_state = BATTERY_STATE.init(Mutex::new(BatteryState {
        charging_current: None,
        battery_voltage: None,
    }));

    spawner
        .spawn(monitor_task(resources.battery_adc, battery_state))
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
        },
    };

    let mut state = AppState::Initialize;

    loop {
        log::info!("New app state: {state:?}");
        state = match state {
            AppState::AdcSetup => adc_setup(&mut board).await,
            AppState::Initialize => initialize(&mut board).await,
            AppState::Measure => measure(&mut board).await,
            AppState::MainMenu => main_menu(&mut board).await,
            AppState::DisplayMenu => display_menu(&mut board).await,
            AppState::Error(error) => app_error(&mut board, error).await,
            AppState::Shutdown => {
                let _ = board.display.shut_down();

                let (_, _, _, touch) = board.frontend.split();
                enter_deep_sleep(touch).await
            }
        };
    }
}

// Debug task, to be removed
#[embassy_executor::task]
async fn monitor_task(mut battery: BatteryAdc, battery_state: &'static SharedBatteryState) {
    let mut timer = Ticker::every(Duration::from_secs(1));

    battery.enable.set_high().unwrap();

    loop {
        let voltage = battery.read_battery_voltage().await;
        let current = battery.read_charge_current().await;

        log::debug!("Voltage = {voltage:?}");
        log::debug!("Current = {current:?}");

        {
            let mut state = battery_state.lock().await;
            state.battery_voltage = voltage.ok();
            state.charging_current = current.ok();
        }

        timer.next().await;
    }
}
