use crate::{board::initialized::Context, AppState};

/// Ensures that the ADC does not keep the touch detector circuit disabled.
/// This state is expected to go away once the ADC can be properly placed into powerdown mode.
pub async fn adc_setup(context: &mut Context) -> AppState {
    unsafe {
        let read_board = core::ptr::read(context);
        let (next_state, new_board) = adc_setup_impl(read_board).await;
        core::ptr::write(context, new_board);
        next_state
    }
}

async fn adc_setup_impl(mut context: Context) -> (AppState, Context) {
    let next_state = match context.frontend.enable_async().await {
        Ok(frontend) => {
            context.frontend = frontend.shut_down().await;
            AppState::PreInitialize
        }
        Err((fe, _err)) => {
            context.frontend = fe;

            context.display_message("ADC error").await;
            AppState::Shutdown
        }
    };

    (next_state, context)
}
