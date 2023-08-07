use alloc::vec::Vec;
use embassy_net::Config;
use embassy_time::{Duration, Instant, Ticker};
use embedded_graphics::Drawable;
use embedded_menu::items::NavigationItem;
use gui::{
    screens::wifi_sta::{WifiStaMenuData, WifiStaMenuEvents, WifiStaMenuScreen},
    widgets::{battery_small::Battery, status_bar::StatusBar},
};

use crate::{board::initialized::Board, states::MIN_FRAME_TIME, AppMenu, AppState};

pub async fn wifi_sta(board: &mut Board) -> AppState {
    // Enable wifi STA. This enabled wifi for the whole menu and re-enables when the user exits the
    // wifi AP config menu.
    board.wifi.initialize(&board.clocks);

    let sta = board
        .wifi
        .configure_sta(Config::dhcpv4(Default::default()))
        .await;

    const MENU_IDLE_DURATION: Duration = Duration::from_secs(30);

    let mut last_interaction = Instant::now();
    let mut ticker = Ticker::every(MIN_FRAME_TIME);

    let mut menu_state = Default::default();

    while last_interaction.elapsed() < MENU_IDLE_DURATION {
        let is_touched = board.frontend.is_touched();
        if is_touched {
            last_interaction = Instant::now();
        }

        // TODO: it would be best if we didn't update the list while interacting with it.
        let networks = sta.visible_networks().await;
        let mut ssids = networks
            .iter()
            .map(|n| NavigationItem::new(&n.ssid, WifiStaMenuEvents::None))
            .collect::<Vec<_>>();
        let mut menu_data = WifiStaMenuData {
            networks: &mut ssids,
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
            },
        };

        if let Some(event) = menu_screen.menu.interact(is_touched) {
            match event {
                WifiStaMenuEvents::None => {}
                WifiStaMenuEvents::Back => return AppState::Menu(AppMenu::Main),
            };
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
