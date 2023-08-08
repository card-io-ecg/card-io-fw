use crate::{
    board::initialized::Board,
    heap::ALLOCATOR,
    states::{AppMenu, MIN_FRAME_TIME},
    AppState,
};
use embassy_time::{Duration, Instant, Ticker};
use embedded_graphics::prelude::*;
use gui::{
    screens::{
        main_menu::{MainMenu, MainMenuEvents, MainMenuScreen},
        MENU_STYLE,
    },
    widgets::{battery_small::Battery, slot::Slot, status_bar::StatusBar},
};

pub async fn main_menu(board: &mut Board) -> AppState {
    const MENU_IDLE_DURATION: Duration = Duration::from_secs(30);

    log::info!("Free heap: {} bytes", ALLOCATOR.free());

    let menu_values = MainMenu {};
    let battery_style = board.config.battery_style();

    let mut menu_screen = MainMenuScreen {
        menu: menu_values.create_menu_with_style(MENU_STYLE),

        status_bar: StatusBar {
            battery: board
                .battery_monitor
                .battery_data()
                .await
                .map(|data| Slot::visible(Battery::with_style(data, battery_style)))
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
                MainMenuEvents::Display => return AppState::Menu(AppMenu::Display),
                MainMenuEvents::WifiSetup => return AppState::WifiAP,
                MainMenuEvents::About => return AppState::Menu(AppMenu::About),
                MainMenuEvents::Shutdown => return AppState::Shutdown,
            };
        }

        let battery_data = board.battery_monitor.battery_data().await;

        menu_screen
            .status_bar
            .update_battery_data(battery_data, battery_style);

        #[cfg(feature = "battery_max17055")]
        if let Some(battery) = battery_data {
            if battery.is_low {
                return AppState::Shutdown;
            }
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
