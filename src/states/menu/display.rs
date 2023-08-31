use crate::{
    board::{
        initialized::{Board, StaMode},
        wifi::sta::Sta,
    },
    states::{AppMenu, TouchInputShaper, MENU_IDLE_DURATION, MIN_FRAME_TIME},
    timeout::Timeout,
    AppState,
};
use embassy_time::Ticker;
use embedded_graphics::prelude::*;
use gui::{
    screens::{
        display_menu::{DisplayMenu, DisplayMenuEvents},
        menu_style,
        screen::Screen,
    },
    widgets::{battery_small::Battery, status_bar::StatusBar, wifi::WifiStateView},
};

pub async fn display_menu(board: &mut Board) -> AppState {
    let sta = if !board.config.known_networks.is_empty() {
        board.enable_wifi_sta(StaMode::OnDemand).await
    } else {
        board.wifi.stop_if().await;
        None
    };

    let mut exit_timer = Timeout::new(MENU_IDLE_DURATION);

    let mut menu_values = DisplayMenu {
        brightness: board.config.display_brightness,
        battery_display: board.config.battery_display_style,
        filter_strength: board.config.filter_strength,
    };

    let mut menu_screen = Screen {
        content: menu_values.create_menu_with_style(menu_style()),

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
                DisplayMenuEvents::Back => {
                    board.save_config().await;
                    return AppState::Menu(AppMenu::Main);
                }
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

        if &menu_values != menu_screen.content.data() {
            log::debug!("Settings changed");
            let new = *menu_screen.content.data();
            if menu_values.brightness != new.brightness {
                board.config_changed = true;
                board.config.display_brightness = new.brightness;
                let _ = board
                    .display
                    .update_brightness_async(board.config.display_brightness())
                    .await;
            }
            if menu_values.battery_display != new.battery_display {
                board.config_changed = true;
                board.config.battery_display_style = new.battery_display;
                menu_screen
                    .status_bar
                    .update_battery_style(board.config.battery_style());
            }
            if menu_values.filter_strength != new.filter_strength {
                board.config_changed = true;
                board.config.filter_strength = new.filter_strength;
            }

            menu_values = new;
        }

        board
            .display
            .frame(|display| {
                menu_screen.content.update(display);
                menu_screen.draw(display)
            })
            .await
            .unwrap();

        ticker.next().await;
    }

    log::info!("Menu timeout");
    board.save_config().await;
    AppState::Shutdown
}
