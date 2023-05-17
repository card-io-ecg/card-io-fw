use embassy_executor::SendSpawner;

use crate::{
    board::{hal::clock::Clocks, EcgFrontend, PoweredDisplay},
    SharedBatteryState,
};

pub struct Board {
    pub display: PoweredDisplay,
    pub frontend: EcgFrontend,
    pub clocks: Clocks<'static>,
    pub high_prio_spawner: SendSpawner,
    pub battery_state: &'static SharedBatteryState,
}
