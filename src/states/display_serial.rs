use crate::{
    board::initialized::Board,
    states::{AppMenu, MIN_FRAME_TIME},
    timeout::Timeout,
    AppState, SerialNumber,
};
use embassy_time::{Duration, Ticker};
use embedded_graphics::Drawable;
use gui::screens::{qr::QrCodeScreen, screen::Screen};
use ufmt::uwrite;

pub async fn display_serial(board: &mut Board) -> AppState {
    let mut ticker = Ticker::every(MIN_FRAME_TIME);
    let mut shutdown_timer = Timeout::new(Duration::from_secs(30));

    let mut serial = heapless::String::<32>::new();
    unwrap!(uwrite!(&mut serial, "Card/IO:{}", SerialNumber::new()));

    while !shutdown_timer.is_elapsed() {
        if board.frontend.is_touched() {
            shutdown_timer.reset();
        }

        if board.battery_monitor.is_low() {
            return AppState::Shutdown;
        }

        let init_screen = Screen {
            content: QrCodeScreen {
                message: serial.as_str(),
            },

            status_bar: board.status_bar(),
        };

        board
            .display
            .frame(|display| init_screen.draw(display))
            .await;

        ticker.next().await;
    }

    AppState::Menu(AppMenu::DeviceInfo)
}
