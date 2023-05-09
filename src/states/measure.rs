use crate::{
    board::initialized::Board, replace_with::replace_with_or_abort_and_return_async, AppState,
};

pub async fn measure(board: &mut Board) -> AppState {
    replace_with_or_abort_and_return_async(board, |mut board| async {
        let frontend = board.frontend.enable_async().await.unwrap();

        board.frontend = frontend.shut_down();

        (AppState::Shutdown, board)
    })
    .await
}
