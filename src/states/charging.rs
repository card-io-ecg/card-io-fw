use crate::{
    board::initialized::Board,
    states::{AppMenu, MIN_FRAME_TIME, TARGET_FPS},
    timeout::Timeout,
    AppState,
};
use embassy_time::{Duration, Ticker};
use embedded_graphics::Drawable;
use gui::screens::charging::ChargingScreen;

pub async fn charging(board: &mut Board) -> AppState {
    const DISPLAY_TIME: Duration = Duration::from_secs(10);

    let mut shutdown_timer = Timeout::new(DISPLAY_TIME);

    let mut charging_screen = ChargingScreen {
        battery_data: board.battery_monitor.battery_data(),
        is_charging: board.battery_monitor.is_charging(),
        frames: 0,
        fps: TARGET_FPS,
        progress: 0,
    };

    let mut ticker = Ticker::every(MIN_FRAME_TIME);
    while board.battery_monitor.is_plugged() && !shutdown_timer.is_elapsed() {
        if board.frontend.is_touched() {
            shutdown_timer.reset();
        }

        if charging_screen.update_touched(board.frontend.is_touched()) {
            return AppState::Menu(AppMenu::Main);
        }

        charging_screen.is_charging = board.battery_monitor.is_charging();
        charging_screen.battery_data = board.battery_monitor.battery_data();
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
