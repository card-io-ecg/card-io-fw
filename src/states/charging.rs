use crate::{
    board::initialized::Context,
    states::{menu::AppMenu, TouchInputShaper, MIN_FRAME_TIME, TARGET_FPS},
    timeout::Timeout,
    AppState,
};
use embassy_time::{Duration, Ticker};
use embedded_graphics::Drawable;
use gui::screens::charging::ChargingScreen;

pub async fn charging(context: &mut Context) -> AppState {
    const DISPLAY_TIME: Duration = Duration::from_secs(10);

    let mut ticker = Ticker::every(MIN_FRAME_TIME);
    let mut exit_timer = Timeout::new(DISPLAY_TIME);

    let mut charging_screen = ChargingScreen {
        battery_data: context.battery_monitor.battery_data(),
        is_charging: context.battery_monitor.is_charging(),
        frames: 0,
        fps: TARGET_FPS,
        progress: 0,
    };

    let mut input = TouchInputShaper::new();
    while context.battery_monitor.is_plugged() && !exit_timer.is_elapsed() {
        input.update(&mut context.frontend);

        let is_touched = input.is_touched();
        if is_touched {
            exit_timer.reset();
        }

        if charging_screen.update_touched(input.is_touched()) {
            return AppState::Menu(AppMenu::Main);
        }

        charging_screen.is_charging = context.battery_monitor.is_charging();
        charging_screen.battery_data = context.battery_monitor.battery_data();
        charging_screen.frames += 1;

        context
            .display
            .frame(|display| charging_screen.draw(display))
            .await;

        ticker.next().await;
    }

    AppState::Shutdown
}
