use crate::{
    board::initialized::Board,
    states::{AppMenu, MIN_FRAME_TIME, TARGET_FPS},
    AppState,
};
use embassy_time::{Duration, Instant, Ticker};
use embedded_graphics::Drawable;
use gui::screens::charging::ChargingScreen;

pub async fn charging(board: &mut Board) -> AppState {
    const DISPLAY_TIME: Duration = Duration::from_secs(10);

    let mut display_started = Instant::now();
    let mut ticker = Ticker::every(MIN_FRAME_TIME);

    let mut charging_screen = ChargingScreen {
        battery_data: board.battery_monitor.battery_data().await,
        is_charging: board.battery_monitor.is_charging(),
        frames: 0,
        fps: TARGET_FPS,
        progress: 0,
    };

    while board.battery_monitor.is_plugged() && display_started.elapsed() <= DISPLAY_TIME {
        if board.frontend.is_touched() {
            display_started = Instant::now();
        }
        if charging_screen.update_touched(board.frontend.is_touched()) {
            return AppState::Menu(AppMenu::Main);
        }

        charging_screen.is_charging = board.battery_monitor.is_charging();
        charging_screen.battery_data = board.battery_monitor.battery_data().await;
        charging_screen.frames += 1;

        board
            .display
            .frame(|display| charging_screen.draw(display))
            .await
            .unwrap();

        ticker.next().await;
    }

    AppState::Shutdown
}
