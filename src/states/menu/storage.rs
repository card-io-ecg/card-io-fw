use crate::{
    board::{
        initialized::{Board, StaMode},
        storage::FileSystem,
        wifi::sta::Sta,
    },
    states::{display_message, AppMenu, TouchInputShaper, MENU_IDLE_DURATION, MIN_FRAME_TIME},
    timeout::Timeout,
    AppState,
};
use embassy_time::Ticker;
use embedded_graphics::prelude::*;
use gui::{
    screens::{
        menu_style,
        screen::Screen,
        storage_menu::{StorageMenu, StorageMenuEvents},
    },
    widgets::{battery_small::Battery, status_bar::StatusBar, wifi::WifiStateView},
};

pub async fn storage_menu(board: &mut Board) -> AppState {
    let sta = board.enable_wifi_sta(StaMode::OnDemand).await;

    let mut exit_timer = Timeout::new(MENU_IDLE_DURATION);

    let mut menu_values = StorageMenu {
        store_measurement: board.config.store_measurement,
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
    let mut input = TouchInputShaper::new();

    while !exit_timer.is_elapsed() {
        input.update(&mut board.frontend);
        let is_touched = input.is_touched();
        if is_touched {
            exit_timer.reset();
        }

        if let Some(event) = menu_screen.content.interact(is_touched) {
            match event {
                StorageMenuEvents::Format => {
                    info!("Format requested");
                    display_message(board, "Formatting storage...").await;
                    core::mem::drop(board.storage.take());
                    FileSystem::format().await;
                    board.storage = FileSystem::mount().await;

                    return AppState::Menu(AppMenu::Main);
                }
                StorageMenuEvents::Upload => {}
                StorageMenuEvents::Back => {
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
            debug!("Settings changed");
            let new = *menu_screen.content.data();
            if menu_values.store_measurement != new.store_measurement {
                board.config_changed = true;
                board.config.store_measurement = new.store_measurement;
            }

            menu_values = new;
        }

        board
            .display
            .frame(|display| {
                menu_screen.content.update(display);
                menu_screen.draw(display)
            })
            .await;

        ticker.next().await;
    }

    info!("Menu timeout");
    board.save_config().await;
    AppState::Shutdown
}
