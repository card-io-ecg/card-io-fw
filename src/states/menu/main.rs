use crate::{
    board::{
        initialized::{Board, StaMode},
        wifi::sta::Sta,
    },
    heap::ALLOCATOR,
    states::{AppMenu, TouchInputShaper, MENU_IDLE_DURATION, MIN_FRAME_TIME},
    timeout::Timeout,
    AppState,
};
use embassy_time::Ticker;
use embedded_graphics::prelude::*;
use embedded_menu::{items::NavigationItem, Menu};
use gui::{
    screens::{menu_style, screen::Screen},
    widgets::{battery_small::Battery, status_bar::StatusBar, wifi::WifiStateView},
};

#[derive(Clone, Copy)]
pub enum MainMenuEvents {
    Display,
    About,
    WifiSetup,
    WifiListVisible,
    Shutdown,
}

pub async fn main_menu(board: &mut Board) -> AppState {
    let sta = if !board.config.known_networks.is_empty() {
        // Enable wifi STA. This enabled wifi for the whole menu and re-enables when the user exits
        // the wifi AP config menu.
        board.enable_wifi_sta(StaMode::OnDemand).await
    } else {
        board.disable_wifi().await;
        None
    };

    let mut exit_timer = Timeout::new(MENU_IDLE_DURATION);
    info!("Free heap: {} bytes", ALLOCATOR.free());

    let builder = Menu::with_style("Main menu", menu_style());

    let mut items = heapless::Vec::<_, 4>::new();

    unwrap!(items
        .push(NavigationItem::new(
            "Display settings",
            MainMenuEvents::Display,
        ))
        .ok());
    unwrap!(items
        .push(NavigationItem::new("Device info", MainMenuEvents::About))
        .ok());

    if board.can_enable_wifi() {
        unwrap!(items
            .push(NavigationItem::new("Wifi setup", MainMenuEvents::WifiSetup))
            .ok());
        unwrap!(items
            .push(NavigationItem::new(
                "Wifi networks",
                MainMenuEvents::WifiListVisible,
            ))
            .ok());
    }

    let mut menu_screen = Screen {
        content: builder
            .add_items(&mut items[..])
            .add_item(NavigationItem::new("Shutdown", MainMenuEvents::Shutdown))
            .build(),

        status_bar: StatusBar {
            battery: Battery::with_style(
                board.battery_monitor.battery_data(),
                board.config.battery_style(),
            ),
            wifi: WifiStateView::new(sta.as_ref().map(Sta::connection_state)),
        },
    };

    let mut ticker = Ticker::every(MIN_FRAME_TIME);
    let mut input = TouchInputShaper::new(&mut board.frontend);

    while !exit_timer.is_elapsed() {
        let is_touched = input.is_touched();
        if is_touched {
            exit_timer.reset();
        }

        if let Some(event) = menu_screen.content.interact(is_touched) {
            match event {
                MainMenuEvents::Display => return AppState::Menu(AppMenu::Display),
                MainMenuEvents::About => return AppState::Menu(AppMenu::DeviceInfo),
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

        unwrap!(board
            .display
            .frame(|display| {
                menu_screen.content.update(display);
                menu_screen.draw(display)
            })
            .await
            .ok());

        ticker.next().await;
    }

    info!("Menu timeout");
    AppState::Shutdown
}
