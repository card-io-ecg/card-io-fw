use crate::{
    board::initialized::Board,
    states::{menu::AppMenu, MIN_FRAME_TIME},
    timeout::Timeout,
    AppState, SerialNumber,
};
use embassy_time::{Duration, Ticker};
use embedded_graphics::Drawable;
use gui::screens::qr::QrCodeScreen;
use ufmt::uwrite;

pub async fn display_serial(board: &mut Board) -> AppState {
    let mut ticker = Ticker::every(MIN_FRAME_TIME);
    let mut shutdown_timer = Timeout::new(Duration::from_secs(30));

    let mut serial = heapless::String::<32>::new();
    unwrap!(uwrite!(&mut serial, "Card/IO:{}", SerialNumber));

    while !shutdown_timer.is_elapsed() {
        if board.frontend.is_touched() {
            shutdown_timer.reset();
        }

        if board.battery_monitor.is_low() {
            return AppState::Shutdown;
        }

        board
            .with_status_bar(|display| {
                QrCodeScreen {
                    message: serial.as_str(),
                    countdown: Some(shutdown_timer.remaining().as_secs() as usize),
                    invert: false,
                }
                .draw(display)
            })
            .await;

        ticker.next().await;
    }

    AppState::Menu(AppMenu::DeviceInfo)
}
