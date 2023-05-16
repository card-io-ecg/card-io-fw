use embassy_executor::SendSpawner;

use crate::board::{
    hal::{self, clock::Clocks},
    startup::StartupResources,
    EcgFrontend, PoweredDisplay,
};

pub struct Board {
    pub display: PoweredDisplay,
    pub frontend: EcgFrontend,
    pub clocks: Clocks<'static>,
    pub high_prio_spawner: SendSpawner,
}

impl Board {
    pub async fn initialize(board: StartupResources) -> Self {
        hal::interrupt::enable(
            hal::peripherals::Interrupt::GPIO,
            hal::interrupt::Priority::Priority3,
        )
        .unwrap();

        Self {
            display: board.display.enable().await.unwrap(),
            frontend: board.frontend,
            clocks: board.clocks,
            high_prio_spawner: board.high_prio_spawner,
        }
    }
}
