use crate::{board::initialized::Board, AppState};

pub async fn measure(board: &mut Board) -> AppState {
    let frontend = board.frontend.enable_async().await.unwrap();

    todo!()
}
