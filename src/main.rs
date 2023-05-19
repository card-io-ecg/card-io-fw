#![no_std]
#![no_main]
#![feature(async_fn_in_trait)]
#![feature(type_alias_impl_trait)]
#![feature(let_chains)]
#![allow(incomplete_features)]

extern crate alloc;

use embassy_executor::{Executor, _export::StaticCell};
use embassy_time::{Duration, Ticker};

use crate::{
    board::{hal::entry, initialized::Board, startup::StartupResources},
    sleep::enter_deep_sleep,
    states::{app_error, display_menu, initialize, main_menu, measure},
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
        spawner.spawn(main_task(resources)).ok();
        spawner.spawn(ticker_task()).ok();
    });
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AppError {
    Adc,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AppState {
    Initialize,
    Measure,
    MainMenu,
    DisplayMenu,
    Error(AppError),
    Shutdown,
}

#[embassy_executor::task]
async fn main_task(resources: StartupResources) {
    // If the device is awake, the display should be enabled.
    let mut board = Board::initialize(resources).await;

    let mut state = AppState::Initialize;

    loop {
        log::info!("New app state: {state:?}");
        state = match state {
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
async fn ticker_task() {
    let mut timer = Ticker::every(Duration::from_secs(1));

    loop {
        timer.next().await;
        log::debug!("Tick");
        timer.next().await;
        log::debug!("Tock");
    }
}
