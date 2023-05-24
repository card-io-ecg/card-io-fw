use embassy_time::{Duration, Instant, Ticker};
use embedded_graphics::Drawable;
use gui::screens::wifi_ap::WifiApScreen;

use crate::{
    board::{initialized::Board, LOW_BATTERY_VOLTAGE},
    states::MIN_FRAME_TIME,
    AppState,
};

pub async fn wifi_ap(board: &mut Board) -> AppState {
    let mut ticker = Ticker::every(MIN_FRAME_TIME);
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(10) {
        let battery_data = board.battery_monitor.battery_data().await;

        if let Some(battery) = battery_data {
            if battery.voltage < LOW_BATTERY_VOLTAGE {
                return AppState::Shutdown;
            }
        }

        let screen = WifiApScreen {
            battery_data,
            battery_style: board.config.battery_style(),
        };

        board
            .display
            .frame(|display| screen.draw(display))
            .await
            .unwrap();

        ticker.next().await;
    }

    AppState::MainMenu
}
