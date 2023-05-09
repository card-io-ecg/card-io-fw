use crate::{
    board::{
        hal::clock::Clocks, startup::StartupResources, AdcDrdy, AdcReset, AdcSpi, DisplayInterface,
        DisplayReset, TouchDetect,
    },
    display::PoweredDisplay,
    frontend::Frontend,
};

pub struct Board {
    pub display: PoweredDisplay<DisplayInterface<'static>, DisplayReset>,
    pub frontend: Frontend<AdcSpi<'static>, AdcDrdy, AdcReset, TouchDetect>,
    pub clocks: Clocks<'static>,
}

impl Board {
    pub async fn initialize(board: StartupResources) -> Self {
        Self {
            display: board.display.enable().await.unwrap(),
            frontend: board.frontend,
            clocks: board.clocks,
        }
    }
}
