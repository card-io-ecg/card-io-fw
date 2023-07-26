use embassy_net::Config;

use crate::{board::initialized::Board, AppMenu, AppState};

pub async fn wifi_sta(board: &mut Board) -> AppState {
    // Enable wifi STA. This enabled wifi for the whole menu and re-enables when the user exits the
    // wifi AP config menu.
    board.wifi.initialize(&board.clocks);

    board
        .wifi
        .configure_sta(Config::dhcpv4(Default::default()))
        .await;

    // Wifi should already be in STA mode here.
    AppState::Menu(AppMenu::Main)
}
