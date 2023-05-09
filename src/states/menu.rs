use crate::{board::initialized::Board, states::MIN_FRAME_TIME, AppState};
use embassy_time::Ticker;
use embedded_graphics::prelude::*;
use gui::screens::{
    main_menu::{MainMenu, MainMenuEvents},
    MENU_STYLE,
};

pub async fn main_menu(board: &mut Board) -> AppState {
    let mut menu = MainMenu {}.create_menu_with_style(MENU_STYLE);

    let mut ticker = Ticker::every(MIN_FRAME_TIME);
    loop {
        if let Some(event) = menu.interact(board.frontend.is_touched()) {
            return match event {
                MainMenuEvents::Shutdown => AppState::Shutdown,
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
}
