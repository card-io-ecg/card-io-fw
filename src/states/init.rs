use crate::{
    board::initialized::Context,
    states::{to_progress, TouchInputShaper, INIT_MENU_THRESHOLD, INIT_TIME, MIN_FRAME_TIME},
    timeout::Timeout,
    AppState,
};
use embassy_time::Ticker;
use embedded_graphics::Drawable;
use gui::screens::init::StartupScreen;

pub async fn initialize(context: &mut Context) -> AppState {
    let mut ticker = Ticker::every(MIN_FRAME_TIME);
    let exit_timer = Timeout::new(INIT_MENU_THRESHOLD);

    let mut input = TouchInputShaper::new_released();
    while !exit_timer.is_elapsed() {
        input.update(&mut context.frontend);

        let is_touched = input.is_touched();
        if !is_touched {
            return AppState::Shutdown;
        }

        if context.battery_monitor.is_low() {
            return AppState::Shutdown;
        }

        let elapsed = exit_timer.elapsed();

        context
            .with_status_bar(|display| {
                StartupScreen {
                    label: "Release to shutdown",
                    progress: to_progress(elapsed, INIT_TIME),
                }
                .draw(display)
            })
            .await;

        ticker.next().await;
    }

    AppState::Measure
}
