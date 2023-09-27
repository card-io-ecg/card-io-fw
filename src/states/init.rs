use crate::{
    board::initialized::Board,
    states::{to_progress, TouchInputShaper, INIT_MENU_THRESHOLD, INIT_TIME, MIN_FRAME_TIME},
    timeout::Timeout,
    AppState,
};
use embassy_time::Ticker;
use embedded_graphics::Drawable;
use gui::screens::{init::StartupScreen, screen::Screen};

pub async fn initialize(board: &mut Board) -> AppState {
    let mut ticker = Ticker::every(MIN_FRAME_TIME);
    let exit_timer = Timeout::new(INIT_MENU_THRESHOLD);

    let mut input = TouchInputShaper::new_released();
    while !exit_timer.is_elapsed() {
        input.update(&mut board.frontend);

        let is_touched = input.is_touched();
        if !is_touched {
            return AppState::Shutdown;
        }

        if board.battery_monitor.is_low() {
            return AppState::Shutdown;
        }

        let elapsed = exit_timer.elapsed();
        let init_screen = Screen {
            content: StartupScreen {
                label: "Release to shutdown",
                progress: to_progress(elapsed, INIT_TIME),
            },

            status_bar: board.status_bar(),
        };

        board
            .display
            .frame(|display| init_screen.draw(display))
            .await;

        ticker.next().await;
    }

    AppState::Measure
}
