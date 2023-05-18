use crate::{
    board::{initialized::Board, BATTERY_MODEL},
    states::MIN_FRAME_TIME,
    AppState,
};
use embassy_time::{Duration, Instant, Ticker};
use embedded_graphics::Drawable;
use gui::screens::charging::ChargingScreen;

pub async fn charging(board: &mut Board) -> AppState {
    const DISPLAY_TIME: Duration = Duration::from_secs(10);

    let mut display_started = Instant::now();
    let mut ticker = Ticker::every(MIN_FRAME_TIME);

    // Count displayed frames since last wakeup
    let mut frames = 0;
    let mut display_active = false;

    while board.battery_monitor.is_plugged() {
        if board.frontend.is_touched() {
            if !display_active {
                frames = 0;
            }

            display_started = Instant::now();
        }

        let elapsed = display_started.elapsed();
        if elapsed <= DISPLAY_TIME {
            display_active = true;
            let battery_data = board.battery_monitor.battery_data().await;
            board
                .display
                .frame(|display| {
                    ChargingScreen {
                        battery_data,
                        model: BATTERY_MODEL,
                        is_charging: board.battery_monitor.is_charging(),
                        frames,
                        fps: 100,
                    }
                    .draw(display)
                })
                .await
                .unwrap();

            frames += 1;
        } else if display_active {
            // Clear display
            board.display.frame(|_display| Ok(())).await.unwrap();
            display_active = false;
        }

        ticker.next().await;
    }

    AppState::Shutdown
}
