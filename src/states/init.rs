use crate::{
    board::initialized::Board,
    states::{AppMenu, MIN_FRAME_TIME},
    timeout::Timeout,
    AppState,
};
use embassy_time::{Duration, Ticker};
use embedded_graphics::Drawable;
use gui::{
    screens::{init::StartupScreen, screen::Screen},
    widgets::{battery_small::Battery, status_bar::StatusBar, wifi::WifiStateView},
};
use signal_processing::lerp::interpolate;

pub async fn initialize(board: &mut Board) -> AppState {
    const INIT_TIME: Duration = Duration::from_secs(4);
    const MENU_THRESHOLD: Duration = Duration::from_secs(2);

    let mut ticker = Ticker::every(MIN_FRAME_TIME);
    let shutdown_timer = Timeout::new(INIT_TIME);
    while !shutdown_timer.is_elapsed() {
        let elapsed = shutdown_timer.elapsed();

        let (on_exit, label) = if elapsed > MENU_THRESHOLD {
            (AppState::Menu(AppMenu::Main), "Release to menu")
        } else {
            (AppState::Shutdown, "Release to shutdown")
        };

        if !board.frontend.is_touched() {
            return on_exit;
        }

        let battery_data = board.battery_monitor.battery_data();

        if let Some(battery) = battery_data {
            if battery.is_low {
                return AppState::Shutdown;
            }
        }

        let init_screen = Screen {
            content: StartupScreen {
                label,
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
            .await
            .unwrap();

        ticker.next().await;
    }

    AppState::Measure
}

fn to_progress(elapsed: Duration, max_duration: Duration) -> u32 {
    interpolate(
        elapsed.as_millis() as u32,
        0,
        max_duration.as_millis() as u32,
        0,
        255,
    )
}
