use crate::{
    board::initialized::Board,
    heap::ALLOCATOR,
    states::{AppMenu, MENU_IDLE_DURATION, MIN_FRAME_TIME},
    timeout::Timeout,
    AppState,
};
use embassy_net::Config;
use embassy_time::Ticker;
use embedded_graphics::prelude::*;
use gui::{
    screens::{
        main_menu::{MainMenu, MainMenuEvents, MainMenuScreen},
        MENU_STYLE,
    },
    widgets::{battery_small::Battery, status_bar::StatusBar},
};

pub async fn main_menu(board: &mut Board) -> AppState {
    if !board.config.known_networks.is_empty() {
        // Enable wifi STA. This enabled wifi for the whole menu and re-enables when the user exits the
        // wifi AP config menu.
        board.wifi.initialize(&board.clocks);

        board
            .wifi
            .configure_sta(Config::dhcpv4(Default::default()))
            .await;
    }

    let mut exit_timer = Timeout::new(MENU_IDLE_DURATION);
    log::info!("Free heap: {} bytes", ALLOCATOR.free());

    let menu_values = MainMenu {};

    let mut menu_screen = MainMenuScreen {
        menu: menu_values.create_menu_with_style(MENU_STYLE),

        status_bar: StatusBar {
            battery: Battery::with_style(
                board.battery_monitor.battery_data(),
                board.config.battery_style(),
            ),
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
