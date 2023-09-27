use crate::{board::initialized::Board, states::menu::AppMenu, AppState};

pub async fn firmware_update(_board: &mut Board) -> AppState {
    AppState::Menu(AppMenu::Main)
}
