use crate::{
    board::{initialized::Board, BATTERY_MODEL},
    AppState,
};
use embassy_time::{Duration, Instant, Ticker};
use embedded_graphics::Drawable;
use gui::screens::charging::ChargingScreen;

pub async fn charging(board: &mut Board) -> AppState {
    const DISPLAY_TIME: Duration = Duration::from_secs(10);
    const FPS: u32 = 10;

    let mut display_started = Instant::now();
    let mut ticker = Ticker::every(Duration::from_hz(FPS as u64));

    // Count displayed frames since last wakeup
    let mut frames = 0;

    while board.battery_monitor.is_plugged() && display_started.elapsed() <= DISPLAY_TIME {
        if board.frontend.is_touched() {
            display_started = Instant::now();
        }

        let battery_data = board.battery_monitor.battery_data().await;
        board
            .display
            .frame(|display| {
                ChargingScreen {
                    battery_data,
                    model: BATTERY_MODEL,
                    is_charging: board.battery_monitor.is_charging(),
                    frames,
                    fps: FPS,
                }
                .draw(display)
            })
            .await
            .unwrap();

        frames += 1;
        ticker.next().await;
    }

    AppState::ShutdownCharging
}
