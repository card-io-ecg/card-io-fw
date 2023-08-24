use alloc::{string::String, vec::Vec};
use embassy_time::{Duration, Ticker};
use embedded_graphics::Drawable;
use embedded_menu::{items::NavigationItem, Menu};
use gui::{
    screens::{menu_style, screen::Screen},
    widgets::{battery_small::Battery, status_bar::StatusBar, wifi::WifiStateView},
};

use crate::{
    board::initialized::{Board, StaMode},
    states::MIN_FRAME_TIME,
    timeout::Timeout,
    AppMenu, AppState,
};

#[derive(Clone, Copy)]
pub enum WifiStaMenuEvents {
    None,
    Back,
}

pub async fn wifi_sta(board: &mut Board) -> AppState {
    let Some(sta) = board.enable_wifi_sta(StaMode::Enable).await else {
        // FIXME: Show error screen
        return AppState::Menu(AppMenu::Main);
    };

    const MENU_IDLE_DURATION: Duration = Duration::from_secs(30);

    let mut ticker = Ticker::every(MIN_FRAME_TIME);
    let mut menu_state = Default::default();
    let mut ssids = Vec::new();

    // Initial placeholder
    ssids.push(NavigationItem::new(
        String::from("Scanning..."),
        WifiStaMenuEvents::None,
    ));

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
                ssids.extend(
                    networks
                        .iter()
                        .map(|n| String::from(n.ssid.as_str()))
                        .map(|n| NavigationItem::new(n, WifiStaMenuEvents::None)),
                );
            }
        }

        let battery_data = board.battery_monitor.battery_data();

        #[cfg(feature = "battery_max17055")]
        if let Some(battery) = battery_data {
            if battery.is_low {
                return AppState::Shutdown;
            }
        }

        let mut menu_screen = Screen {
            content: Menu::with_style("Access points", menu_style())
                .add_items(&mut ssids)
                .add_item(NavigationItem::new("Back", WifiStaMenuEvents::Back))
                .build_with_state(menu_state),

            status_bar: StatusBar {
                battery: Battery::with_style(battery_data, board.config.battery_style()),
                wifi: WifiStateView::enabled(sta.connection_state()),
            },
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
            .await
            .unwrap();

        menu_state = menu_screen.content.state();

        ticker.next().await;
    }

    AppState::Menu(AppMenu::Main)
}
