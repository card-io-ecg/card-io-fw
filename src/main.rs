#![no_std]
#![no_main]
#![feature(async_fn_in_trait)]
#![feature(type_alias_impl_trait)]
#![feature(let_chains)]
#![allow(incomplete_features)]

extern crate alloc;

use embassy_executor::{Executor, _export::StaticCell};

use crate::{
    board::{hal::entry, initialized::Board, startup::StartupResources},
    sleep::enter_deep_sleep,
    states::{app_error, initialize, main_menu, measure},
};

mod board;
mod display;
mod frontend;
mod heap;
mod replace_with;
mod sleep;
mod spi_device;
mod states;

static EXECUTOR: StaticCell<Executor> = StaticCell::new();

#[entry]
fn main() -> ! {
    // Board::initialize initialized embassy so it must be called first.
    let resources = StartupResources::initialize();

    let executor = EXECUTOR.init(Executor::new());
    executor.run(move |spawner| {
        spawner.spawn(main_task(resources)).ok();
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
            AppState::Error(error) => app_error(&mut board, error).await,
            AppState::Shutdown => {
                let _ = board.display.shut_down();

                let (_, _, _, touch) = board.frontend.split();
                enter_deep_sleep(touch).await
            }
        };
    }
}
