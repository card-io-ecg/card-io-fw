use crate::{
    board::{initialized::Board, wifi::sta::Sta},
    heap::ALLOCATOR,
    states::{AppMenu, MENU_IDLE_DURATION, MIN_FRAME_TIME},
    timeout::Timeout,
    AppState,
};
use embassy_time::Ticker;
use embedded_graphics::prelude::*;
use gui::{
    screens::main_menu::{MainMenuData, MainMenuEvents, MainMenuScreen},
    widgets::{battery_small::Battery, status_bar::StatusBar, wifi::WifiStateView},
};

pub async fn main_menu(board: &mut Board) -> AppState {
    let sta = if !board.config.known_networks.is_empty() {
        // Enable wifi STA. This enabled wifi for the whole menu and re-enables when the user exits
        // the wifi AP config menu.
        Some(board.enable_wifi_sta().await)
    } else {
        board.disable_wifi().await;
        None
    };

    let mut exit_timer = Timeout::new(MENU_IDLE_DURATION);
    log::info!("Free heap: {} bytes", ALLOCATOR.free());

    let menu_data = MainMenuData {};

    let mut menu_screen = MainMenuScreen {
        menu: menu_data.create_menu(),

        status_bar: StatusBar {
            battery: Battery::with_style(
                board.battery_monitor.battery_data(),
                board.config.battery_style(),
            ),
            wifi: WifiStateView::new(sta.as_ref().map(Sta::connection_state)),
        },
    };

    let mut ticker = Ticker::every(MIN_FRAME_TIME);

    while !exit_timer.is_elapsed() {
        let is_touched = board.frontend.is_touched();
        if is_touched {
            exit_timer.reset();
        }

        if let Some(event) = menu_screen.menu.interact(is_touched) {
            match event {
                MainMenuEvents::Display => return AppState::Menu(AppMenu::Display),
                MainMenuEvents::About => return AppState::Menu(AppMenu::About),
                MainMenuEvents::WifiSetup => return AppState::Menu(AppMenu::WifiAP),
                MainMenuEvents::WifiListVisible => return AppState::Menu(AppMenu::WifiListVisible),
                MainMenuEvents::Shutdown => return AppState::Shutdown,
            };
        }

        let battery_data = board.battery_monitor.battery_data();

        #[cfg(feature = "battery_max17055")]
        if let Some(battery) = battery_data {
            if battery.is_low {
                return AppState::Shutdown;
            }
        }

        menu_screen.status_bar.update_battery_data(battery_data);
        if let Some(ref sta) = sta {
            menu_screen.status_bar.wifi.update(sta.connection_state());
        }

        board
            .display
            .frame(|display| {
                menu_screen.menu.update(display);
                menu_screen.draw(display)
            })
            .await
            .unwrap();

        ticker.next().await;
    }

    log::info!("Menu timeout");
    AppState::Shutdown
}
