#![no_std]
#![no_main]
#![feature(async_fn_in_trait)]
#![feature(type_alias_impl_trait)]
#![feature(let_chains)]
#![allow(incomplete_features)]

extern crate alloc;

use embassy_executor::{Executor, Spawner, _export::StaticCell};
use embassy_time::{Duration, Ticker};
use embedded_hal::digital::OutputPin;

use crate::{
    board::{
        hal::{self, entry},
        initialized::Board,
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

static EXECUTOR: StaticCell<Executor> = StaticCell::new();

#[entry]
fn main() -> ! {
    // Board::initialize initialized embassy so it must be called first.
    let resources = StartupResources::initialize();

    let executor = EXECUTOR.init(Executor::new());
    executor.run(move |spawner| {
        spawner.spawn(main_task(spawner, resources)).ok();
    });
}

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

#[embassy_executor::task]
async fn main_task(spawner: Spawner, resources: StartupResources) {
    spawner.spawn(ticker_task(resources.battery_adc)).ok();

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
async fn ticker_task(mut battery: BatteryAdc) {
    let mut timer = Ticker::every(Duration::from_secs(1));

    battery.enable.set_high().unwrap();

    loop {
        let voltage = battery.read_battery_voltage().await;
        let current = battery.read_charge_current().await;

        log::debug!("Voltage = {voltage:?}");
        log::debug!("Current = {current:?}");

        timer.next().await;
    }
}
