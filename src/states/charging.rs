use crate::{
    board::{initialized::Board, BATTERY_MODEL},
    AppState,
};
use embassy_futures::select::select;
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
                        fps: FPS,
                    }
                    .draw(display)
                })
                .await
                .unwrap();

            frames += 1;
            ticker.next().await;
        } else if display_active {
            // Clear display
            board.display.frame(|_display| Ok(())).await.unwrap();
            display_active = false;

            log::debug!("Sleeping");
            select(
                board.battery_monitor.wait_for_unplugged(),
                board.frontend.wait_for_touch(),
            )
            .await;
            log::debug!("Wakeup");
            ticker = Ticker::every(Duration::from_hz(FPS as u64));
            if !display_active {
                frames = 0;
            }

            display_started = Instant::now();
        }
    }

    AppState::Shutdown
}
