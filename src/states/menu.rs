use crate::{board::initialized::Board, states::MIN_FRAME_TIME, AppState};
use embassy_time::{Duration, Instant, Ticker};
use embedded_graphics::prelude::*;
use gui::screens::{
    main_menu::{MainMenu, MainMenuEvents},
    MENU_STYLE,
};

pub async fn main_menu(board: &mut Board) -> AppState {
    const MENU_IDLE_DURATION: Duration = Duration::from_secs(30);

    let mut menu = MainMenu {}.create_menu_with_style(MENU_STYLE);

    let mut last_interaction = Instant::now();
    let mut ticker = Ticker::every(MIN_FRAME_TIME);

    while last_interaction.elapsed() < MENU_IDLE_DURATION {
        let is_touched = board.frontend.is_touched();
        if is_touched {
            last_interaction = Instant::now();
        }
        if let Some(event) = menu.interact(is_touched) {
            match event {
                MainMenuEvents::WifiSetup => {}
                MainMenuEvents::Shutdown => return AppState::Shutdown,
            };
        }

        board
            .display
            .frame(|display| {
                menu.update(display);
                menu.draw(display)
            })
            .await
            .unwrap();

        ticker.next().await;
    }

    log::info!("Menu timeout");
    AppState::Shutdown
}
