use crate::{
    board::initialized::Board,
    states::{to_progress, MIN_FRAME_TIME},
    timeout::Timeout,
    AppState,
};
use embassy_time::{Duration, Ticker};
use embedded_graphics::Drawable;
use gui::{
    screens::{init::StartupScreen, screen::Screen},
    widgets::{battery_small::Battery, status_bar::StatusBar, wifi::WifiStateView},
};

pub async fn initialize(board: &mut Board) -> AppState {
    const INIT_TIME: Duration = Duration::from_secs(4);
    const MENU_THRESHOLD: Duration = Duration::from_secs(2);

    let mut ticker = Ticker::every(MIN_FRAME_TIME);
    let shutdown_timer = Timeout::new(MENU_THRESHOLD);
    while !shutdown_timer.is_elapsed() {
        let elapsed = shutdown_timer.elapsed();

        if !board.frontend.is_touched() {
            return AppState::Shutdown;
        }

        let battery_data = board.battery_monitor.battery_data();

        if let Some(battery) = battery_data {
            if battery.is_low {
                return AppState::Shutdown;
            }
        }

        let init_screen = Screen {
            content: StartupScreen {
                label: "Release to shutdown",
                progress: to_progress(elapsed, INIT_TIME),
            },

            status_bar: StatusBar {
                battery: Battery::with_style(battery_data, board.config.battery_style()),
                wifi: WifiStateView::disabled(),
            },
        };

        board
            .display
            .frame(|display| init_screen.draw(display))
            .await;

        ticker.next().await;
    }

    AppState::Measure
}
