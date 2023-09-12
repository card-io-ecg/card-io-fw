use crate::{
    board::initialized::Board,
    states::{AppMenu, TouchInputShaper, MENU_IDLE_DURATION, MIN_FRAME_TIME},
    timeout::Timeout,
    AppState,
};
use embassy_time::Ticker;
use embedded_graphics::prelude::*;
use gui::screens::{
    display_menu::{DisplayMenu, DisplayMenuEvents},
    menu_style,
    screen::Screen,
};

pub async fn display_menu(board: &mut Board) -> AppState {
    let mut exit_timer = Timeout::new(MENU_IDLE_DURATION);

    let mut menu_values = DisplayMenu {
        brightness: board.config.display_brightness,
        battery_display: board.config.battery_display_style,
        filter_strength: board.config.filter_strength,
    };

    let mut menu_screen = Screen {
        content: menu_values.create_menu_with_style(menu_style()),

        status_bar: board.status_bar(),
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
                DisplayMenuEvents::Back => {
                    board.save_config().await;
                    return AppState::Menu(AppMenu::Main);
                }
            };
        }

        #[cfg(feature = "battery_max17055")]
        if board.battery_monitor.is_low() {
            return AppState::Shutdown;
        }

        menu_screen.status_bar = board.status_bar();

        if &menu_values != menu_screen.content.data() {
            debug!("Settings changed");
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
            .await;

        ticker.next().await;
    }

    info!("Menu timeout");
    board.save_config().await;
    AppState::Shutdown
}
