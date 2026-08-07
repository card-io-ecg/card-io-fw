use crate::{
    board::initialized::Context,
    states::{menu::AppMenu, MIN_FRAME_TIME},
    timeout::Timeout,
    AppState, SerialNumber,
};
use embassy_time::{Duration, Ticker};
use embedded_graphics::Drawable;
use gui::screens::qr::QrCodeScreen;
use ufmt::uwrite;

pub async fn display_serial(context: &mut Context) -> AppState {
    let mut ticker = Ticker::every(MIN_FRAME_TIME);
    let mut shutdown_timer = Timeout::new(Duration::from_secs(30));

    let mut serial = heapless::String::<32>::new();
    unwrap!(uwrite!(&mut serial, "Card/IO:{}", SerialNumber));
    let mut previous_secs = 0;

    while !shutdown_timer.is_elapsed() {
        if context.frontend.is_touched() {
            shutdown_timer.reset();
        }

        if context.battery_monitor.is_low() {
            return AppState::Shutdown;
        }

        let secs = shutdown_timer.remaining().as_secs() as usize;
        context
            .with_status_bar(|display| {
                if secs != previous_secs {
                    previous_secs = secs;
                    QrCodeScreen {
                        message: serial.as_str(),
                        countdown: Some(secs),
                        invert: false,
                    }
                    .draw(display)?;
                    Ok(true)
                } else {
                    Ok(false)
                }
            })
            .await;

        ticker.next().await;
    }

    AppState::Menu(AppMenu::DeviceInfo)
}
