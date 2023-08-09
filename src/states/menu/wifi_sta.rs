use alloc::{string::String, vec::Vec};
use embassy_time::{Duration, Ticker};
use embedded_graphics::Drawable;
use embedded_menu::items::NavigationItem;
use gui::{
    screens::wifi_sta::{WifiStaMenuData, WifiStaMenuEvents, WifiStaMenuScreen},
    widgets::{battery_small::Battery, status_bar::StatusBar, wifi::WifiStateView},
};

use crate::{
    board::initialized::Board, states::MIN_FRAME_TIME, timeout::Timeout, AppMenu, AppState,
};

pub async fn wifi_sta(board: &mut Board) -> AppState {
    let sta = board.enable_wifi_sta().await;

    const MENU_IDLE_DURATION: Duration = Duration::from_secs(30);

    let mut ticker = Ticker::every(MIN_FRAME_TIME);
    let mut menu_state = Default::default();
    let mut ssids = Vec::new();

    // Initial placeholder
    ssids.push(String::from("Scanning..."));

    let mut released = false;

    let mut exit_timer = Timeout::new(MENU_IDLE_DURATION);
    while !exit_timer.is_elapsed() {
        let is_touched = board.frontend.is_touched();
        if is_touched {
            exit_timer.reset();
        } else {
            released = true;
        }

        if !is_touched || !released {
            let networks = sta.visible_networks().await;

            if released || !networks.is_empty() {
                ssids.clear();
                ssids.extend(networks.iter().map(|n| n.ssid.as_str()).map(String::from));
            }
        }

        let mut ssid_items = ssids
            .iter()
            .map(|n| NavigationItem::new(n, WifiStaMenuEvents::None))
            .collect::<Vec<_>>();
        let mut menu_data = WifiStaMenuData {
            networks: &mut ssid_items,
        };

        let battery_data = board.battery_monitor.battery_data();

        #[cfg(feature = "battery_max17055")]
        if let Some(battery) = battery_data {
            if battery.is_low {
                return AppState::Shutdown;
            }
        }

        let mut menu_screen = WifiStaMenuScreen {
            menu: menu_data.create(menu_state),
            status_bar: StatusBar {
                battery: Battery::with_style(battery_data, board.config.battery_style()),
                wifi: WifiStateView::enabled(sta.connection_state()),
            },
        };

        if let Some(WifiStaMenuEvents::Back) = menu_screen.menu.interact(is_touched) {
            return AppState::Menu(AppMenu::Main);
        }

        board
            .display
            .frame(|display| {
                menu_screen.menu.update(display);
                menu_screen.draw(display)
            })
            .await
            .unwrap();

        menu_state = menu_screen.menu.state();

        ticker.next().await;
    }

    AppState::Menu(AppMenu::Main)
}
