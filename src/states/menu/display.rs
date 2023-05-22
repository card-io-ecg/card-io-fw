use crate::{
    board::{
        initialized::Board, BATTERY_MODEL, DEFAULT_BATTERY_DISPLAY_STYLE, LOW_BATTERY_VOLTAGE,
    },
    states::MIN_FRAME_TIME,
    AppState,
};
use embassy_time::{Duration, Instant, Ticker};
use embedded_graphics::prelude::*;
use gui::{
    screens::{
        display_menu::{DisplayBrightness, DisplayMenu, DisplayMenuEvents, DisplayMenuScreen},
        MENU_STYLE,
    },
    widgets::battery_small::BatteryStyle,
};
use ssd1306::prelude::Brightness;

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
        battery_display: DEFAULT_BATTERY_DISPLAY_STYLE,
    };

    let mut menu_screen = DisplayMenuScreen {
        menu: menu_values.create_menu_with_style(MENU_STYLE),

        battery_data,
        battery_style: BatteryStyle::new(DEFAULT_BATTERY_DISPLAY_STYLE, BATTERY_MODEL),
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
                let _ = board
                    .display
                    .update_brightness_async(match new.brightness {
                        DisplayBrightness::Dimmest => Brightness::DIMMEST,
                        DisplayBrightness::Dim => Brightness::DIM,
                        DisplayBrightness::Normal => Brightness::NORMAL,
                        DisplayBrightness::Bright => Brightness::BRIGHT,
                        DisplayBrightness::Brightest => Brightness::BRIGHTEST,
                    })
                    .await;
            }
            if menu_values.battery_display != new.battery_display {
                // TODO: store on exit (note: 2 exit sites)
                menu_screen.battery_style = BatteryStyle::new(new.battery_display, BATTERY_MODEL);
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
