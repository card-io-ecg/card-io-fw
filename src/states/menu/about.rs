use crate::{board::initialized::Board, AppState};

use super::AppMenu;

pub async fn about_menu(_board: &mut Board) -> AppState {
    AppState::Menu(AppMenu::Main)
}
