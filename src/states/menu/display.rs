use crate::{
    board::{initialized::Board, LOW_BATTERY_VOLTAGE},
    states::MIN_FRAME_TIME,
    AppState,
};
use embassy_time::{Duration, Instant, Ticker};
use embedded_graphics::prelude::*;
use gui::screens::{
    display_menu::{DisplayBrightness, DisplayMenu, DisplayMenuEvents, DisplayMenuScreen},
    MENU_STYLE,
};

pub async fn display_menu(board: &mut Board) -> AppState {
    const MENU_IDLE_DURATION: Duration = Duration::from_secs(30);

    let battery_data = board.battery_monitor.battery_data().await;

    if let Some(battery) = battery_data {
        if battery.voltage < LOW_BATTERY_VOLTAGE {
            return AppState::Shutdown;
        }
    }
    let mut menu_values = DisplayMenu {
        // TODO: read from some storage
        brightness: DisplayBrightness::Normal,
        battery_display: board.config.battery_display_style,
    };

    let mut menu_screen = DisplayMenuScreen {
        menu: menu_values.create_menu_with_style(MENU_STYLE),

        battery_data,
        battery_style: board.config.battery_style(),
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
                DisplayMenuEvents::Back => return AppState::MainMenu,
            };
        }

        menu_screen.battery_data = board.battery_monitor.battery_data().await;

        if &menu_values != menu_screen.menu.data() {
            log::debug!("Settings changed");
            let new = *menu_screen.menu.data();
            if menu_values.brightness != new.brightness {
                // TODO: store on exit (note: 2 exit sites)
                board.config.display_brightness = new.brightness;
                let _ = board
                    .display
                    .update_brightness_async(board.config.display_brightness())
                    .await;
            }
            if menu_values.battery_display != new.battery_display {
                // TODO: store on exit (note: 2 exit sites)
                board.config.battery_display_style = new.battery_display;
                menu_screen.battery_style = board.config.battery_style();
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
    AppState::Shutdown
}
