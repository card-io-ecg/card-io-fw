use crate::{
    board::initialized::Board, replace_with::replace_with_or_abort_and_return_async,
    states::display_message, AppState,
};

/// Ensures that the ADC does not keep the touch detector circuit disabled.
/// This state is expected to go away once the ADC can be properly placed into powerdown mode.
pub async fn adc_setup(board: &mut Board) -> AppState {
    replace_with_or_abort_and_return_async(board, |mut board| async {
        match board.frontend.enable_async().await {
            Ok(frontend) => {
                board.frontend = frontend.shut_down().await;
                let next_state = if board.battery_monitor.is_plugged() {
                    AppState::Charging
                } else {
                    AppState::Initialize
                };
                (next_state, board)
            }
            Err((fe, _err)) => {
                board.frontend = fe;

                display_message(&mut board, "ADC error").await;
                (AppState::Shutdown, board)
            }
        }
    })
    .await
}
