use crate::{
    board::{
        initialized::{Context, InnerContext},
        EcgFrontend,
    },
    AppState,
};

/// Ensures that the ADC does not keep the touch detector circuit disabled.
/// This state is expected to go away once the ADC can be properly placed into powerdown mode.
pub async fn adc_setup(context: &mut Context) -> AppState {
    unsafe {
        let frontend = core::ptr::read(&context.frontend);

        let (next_state, frontend) = adc_setup_impl(&mut context.inner, frontend).await;

        core::ptr::write(&mut context.frontend, frontend);
        next_state
    }
}

async fn adc_setup_impl(
    context: &mut InnerContext,
    frontend: EcgFrontend,
) -> (AppState, EcgFrontend) {
    match frontend.enable_async().await {
        Ok(frontend) => (AppState::PreInitialize, frontend.shut_down().await),
        Err((frontend, _err)) => {
            context.display_message("ADC error").await;
            (AppState::Shutdown, frontend)
        }
    }
}
