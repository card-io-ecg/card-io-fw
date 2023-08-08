use crate::{
    board::initialized::Board,
    states::{AppMenu, MIN_FRAME_TIME},
    AppState,
};
use embassy_time::{Duration, Instant, Ticker};
use embedded_graphics::prelude::*;
use gui::{
    screens::{
        display_menu::{DisplayMenu, DisplayMenuEvents, DisplayMenuScreen},
        MENU_STYLE,
    },
    widgets::{battery_small::Battery, slot::Slot, status_bar::StatusBar},
};

pub async fn display_menu(board: &mut Board) -> AppState {
    const MENU_IDLE_DURATION: Duration = Duration::from_secs(30);

    let mut menu_values = DisplayMenu {
        brightness: board.config.display_brightness,
        battery_display: board.config.battery_display_style,
    };

    let mut menu_screen = DisplayMenuScreen {
        menu: menu_values.create_menu_with_style(MENU_STYLE),

        status_bar: StatusBar {
            battery: board
                .battery_monitor
                .battery_data()
                .await
                .map(|data| Slot::visible(Battery::with_style(data, board.config.battery_style())))
                .unwrap_or_default(),
        },
    };

    let mut last_interaction = Instant::now();
    let mut ticker = Ticker::every(MIN_FRAME_TIME);

    while last_interaction.elapsed() < MENU_IDLE_DURATION {
        let is_touched = board.frontend.is_touched();
        if is_touched {
            last_interaction = Instant::now();
        }
        if let Some(event) = menu_screen.menu.interact(is_touched) {
            match event {
                DisplayMenuEvents::Back => {
                    board.save_config().await;
                    return AppState::Menu(AppMenu::Main);
                }
            };
        }

        let battery_data = board.battery_monitor.battery_data().await;

        menu_screen
            .status_bar
            .update_battery_data(battery_data, board.config.battery_style());

        #[cfg(feature = "battery_max17055")]
        if let Some(battery) = battery_data {
            if battery.is_low {
                return AppState::Shutdown;
            }
        }

        if &menu_values != menu_screen.menu.data() {
            log::debug!("Settings changed");
            let new = *menu_screen.menu.data();
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

            menu_values = new;
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
    board.save_config().await;
    AppState::Shutdown
}
