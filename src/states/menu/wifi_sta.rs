use alloc::{string::String, vec::Vec};
use embassy_time::{Duration, Ticker};
use embedded_graphics::Drawable;
use embedded_menu::items::NavigationItem;
use gui::screens::{create_menu, screen::Screen};

use crate::{
    board::initialized::Board,
    states::{TouchInputShaper, MIN_FRAME_TIME},
    timeout::Timeout,
    AppMenu, AppState,
};

#[derive(Clone, Copy)]
pub enum WifiStaMenuEvents {
    None,
    Back,
}

pub async fn wifi_sta(board: &mut Board) -> AppState {
    let Some(sta) = board.enable_wifi_sta_for_scan().await else {
        // FIXME: Show error screen
        return AppState::Menu(AppMenu::Main);
    };

    const MENU_IDLE_DURATION: Duration = Duration::from_secs(30);
    const SCAN_IDLE_DURATION: Duration = Duration::from_secs(1);

    let mut ticker = Ticker::every(MIN_FRAME_TIME);
    let mut menu_state = Default::default();
    let mut ssids = Vec::new();

    let list_item = |label: &str| NavigationItem::new(String::from(label), WifiStaMenuEvents::None);

    // Initial placeholder
    ssids.push(list_item("Scanning..."));

    let mut exit_timer = Timeout::new(MENU_IDLE_DURATION);
    let mut scan_idle_timer = Timeout::new(Duration::from_millis(0));

    let mut input = TouchInputShaper::new();
    while !exit_timer.is_elapsed() {
        input.update(&mut board.frontend);
        let is_touched = input.is_touched();

        if scan_idle_timer.is_elapsed() {
            scan_idle_timer = Timeout::new(SCAN_IDLE_DURATION);
            let networks = sta.visible_networks().await;

            if !networks.is_empty() {
                ssids.clear();
                ssids.extend(networks.iter().map(|n| list_item(&n.ssid)));
            }
        }

        if is_touched {
            scan_idle_timer.reset();
            exit_timer.reset();
        }

        #[cfg(feature = "battery_max17055")]
        if board.battery_monitor.is_low() {
            return AppState::Shutdown;
        }

        let mut menu_screen = Screen {
            content: create_menu("Access points")
                .add_items(&mut ssids)
                .add_item(NavigationItem::new("Back", WifiStaMenuEvents::Back))
                .build_with_state(menu_state),

            status_bar: board.status_bar(),
        };

        if let Some(WifiStaMenuEvents::Back) = menu_screen.content.interact(is_touched) {
            return AppState::Menu(AppMenu::Main);
        }

        board
            .display
            .frame(|display| {
                menu_screen.content.update(display);
                menu_screen.draw(display)
            })
            .await;

        menu_state = menu_screen.content.state();

        ticker.next().await;
    }

    AppState::Menu(AppMenu::Main)
}
