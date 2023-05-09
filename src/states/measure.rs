use crate::{
    board::{AdcDrdy, AdcReset, AdcSpi, DisplayInterface, DisplayReset, TouchDetect},
    display,
    frontend::Frontend,
    AppState,
};

pub async fn measure(
    display: &mut display::PoweredDisplay<'_, DisplayInterface<'_>, DisplayReset>,
    frontend: &mut Frontend<AdcSpi<'_>, AdcDrdy, AdcReset, TouchDetect>,
) -> AppState {
    let frontend = frontend.enable_async().await.unwrap();

    todo!()
}
