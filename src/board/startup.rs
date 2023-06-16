use embassy_executor::SendSpawner;

use crate::board::{
    hal::{clock::Clocks, system::PeripheralClockControl},
    wifi_driver::WifiDriver,
    BatteryAdc, Display, EcgFrontend, MiscPins,
};

pub struct StartupResources {
    pub display: Display,
    pub frontend: EcgFrontend,
    pub clocks: Clocks<'static>,
    pub peripheral_clock_control: PeripheralClockControl,
    pub battery_adc: BatteryAdc,
    pub misc_pins: MiscPins,
    pub high_prio_spawner: SendSpawner,
    pub wifi: WifiDriver,
}
