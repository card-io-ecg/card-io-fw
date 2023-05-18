use crate::{
    board::{initialized::Board, BATTERY_MODEL},
    states::MIN_FRAME_TIME,
    AppState,
};
use embassy_time::{Duration, Instant, Ticker};
use embedded_graphics::prelude::*;
use gui::{
    screens::{
        main_menu::{MainMenu, MainMenuEvents, MainMenuScreen},
        MENU_STYLE,
    },
    widgets::battery_small::BatteryStyle,
};

pub async fn main_menu(board: &mut Board) -> AppState {
    const MENU_IDLE_DURATION: Duration = Duration::from_secs(30);

    let menu_values = MainMenu {};

    let mut menu_screen = MainMenuScreen {
        menu: menu_values.create_menu_with_style(MENU_STYLE),
        battery_data: board.battery_monitor.battery_data().await,
        battery_style: BatteryStyle::Icon(BATTERY_MODEL),
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
                MainMenuEvents::Display => return AppState::DisplayMenu,
                MainMenuEvents::WifiSetup => {}
                MainMenuEvents::Shutdown => return AppState::Shutdown,
            };
        }

        menu_screen.battery_data = board.battery_monitor.battery_data().await;

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
