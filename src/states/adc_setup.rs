use crate::{board::initialized::Board, states::display_message, AppState};

/// Ensures that the ADC does not keep the touch detector circuit disabled.
/// This state is expected to go away once the ADC can be properly placed into powerdown mode.
pub async fn adc_setup(board: &mut Board) -> AppState {
    unsafe {
        let read_board = core::ptr::read(board);
        let (next_state, new_board) = adc_setup_impl(read_board).await;
        core::ptr::write(board, new_board);
        next_state
    }
}

async fn adc_setup_impl(mut board: Board) -> (AppState, Board) {
    let next_state = match board.frontend.enable_async().await {
        Ok(frontend) => {
            board.frontend = frontend.shut_down().await;
            AppState::PreInitialize
        }
        Err((fe, _err)) => {
            board.frontend = fe;

            display_message(&mut board, "ADC error").await;
            AppState::Shutdown
        }
    };

    (next_state, board)
}
