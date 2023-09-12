use crate::{
    board::initialized::Board,
    states::{to_progress, MIN_FRAME_TIME},
    timeout::Timeout,
    AppState,
};
use embassy_time::{Duration, Ticker};
use embedded_graphics::Drawable;
use gui::screens::{init::StartupScreen, screen::Screen};

pub const INIT_TIME: Duration = Duration::from_millis(3000);
pub const MENU_THRESHOLD: Duration = Duration::from_millis(1500);

pub async fn initialize(board: &mut Board) -> AppState {
    let mut ticker = Ticker::every(MIN_FRAME_TIME);
    let shutdown_timer = Timeout::new(MENU_THRESHOLD);

    while !shutdown_timer.is_elapsed() {
        let elapsed = shutdown_timer.elapsed();

        if !board.frontend.is_touched() {
            return AppState::Shutdown;
        }

        if let Some(battery) = board.battery_monitor.battery_data() {
            if battery.is_low {
                return AppState::Shutdown;
            }
        }

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
