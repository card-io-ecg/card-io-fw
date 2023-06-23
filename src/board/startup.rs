use embassy_executor::SendSpawner;

#[cfg(feature = "battery_adc")]
use crate::board::BatteryAdc;
use crate::board::{
    hal::{clock::Clocks, system::PeripheralClockControl},
    wifi_driver::WifiDriver,
    Display, EcgFrontend, MiscPins,
};

pub struct StartupResources {
    pub display: Display,
    pub frontend: EcgFrontend,
    pub clocks: Clocks<'static>,
    pub peripheral_clock_control: PeripheralClockControl,
    #[cfg(feature = "battery_adc")]
    pub battery_adc: BatteryAdc,
    pub misc_pins: MiscPins,
    pub high_prio_spawner: SendSpawner,
    pub wifi: WifiDriver,
}
