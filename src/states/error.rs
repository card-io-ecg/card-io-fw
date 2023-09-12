use crate::{board::initialized::Board, states::display_message_while_touched, AppError, AppState};

pub async fn app_error(board: &mut Board, error: AppError) -> AppState {
    let message = match error {
        AppError::Adc => "ADC is not working",
    };
    display_message_while_touched(board, message).await;

    AppState::Shutdown
}
