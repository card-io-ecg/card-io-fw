use crate::{board::initialized::Board, states::MIN_FRAME_TIME, AppState};
use embassy_time::{Duration, Instant, Ticker};
use embedded_graphics::Drawable;
use gui::{screens::init::StartupScreen, widgets::battery_small::BatteryStyle};
use signal_processing::battery::BatteryModel;

pub async fn initialize(board: &mut Board) -> AppState {
    const INIT_TIME: Duration = Duration::from_secs(4);
    const MENU_THRESHOLD: Duration = Duration::from_secs(2);

    let entered = Instant::now();
    let mut ticker = Ticker::every(MIN_FRAME_TIME);
    while let elapsed = entered.elapsed() && elapsed <= INIT_TIME {
        if !board.frontend.is_touched() {
            return if elapsed > MENU_THRESHOLD {
                AppState::MainMenu
            } else {
                AppState::Shutdown
            };
        }

        let battery_data = board.battery_monitor.battery_data().await;

        board.display
            .frame(|display| {
                let elapsed_secs = elapsed.as_millis() as u32;
                let max_secs = (INIT_TIME.as_millis() as u32).max(elapsed_secs);

                let max_progress = 255;
                let progress = (elapsed_secs * max_progress) / max_secs;

                StartupScreen {
                    label: if elapsed > MENU_THRESHOLD {
                        "Release to menu"
                    } else {
                        "Release to shutdown"
                    },
                    progress,
                    max_progress,
                    battery_data,
                    battery_style: BatteryStyle::Percentage(BatteryModel { voltage: (2750, 4200), charge_current: (0, 1000) })
                }
                .draw(display)
            })
            .await
            .unwrap();

        ticker.next().await;
    }

    AppState::Measure
}
