use embassy_executor::SendSpawner;

#[cfg(feature = "battery_max17055")]
use max17055::Max17055;

#[cfg(feature = "battery_adc")]
use crate::board::BatteryAdc;
#[cfg(feature = "battery_max17055")]
use crate::board::BatteryFgI2c;
use crate::board::{
    hal::{clock::Clocks, system::PeripheralClockControl},
    wifi::driver::WifiDriver,
    Display, EcgFrontend, MiscPins,
};

pub struct StartupResources {
    pub display: Display,
    pub frontend: EcgFrontend,
    pub clocks: Clocks<'static>,
    pub peripheral_clock_control: PeripheralClockControl,
    #[cfg(feature = "battery_adc")]
    pub battery_adc: BatteryAdc,

    #[cfg(feature = "battery_max17055")]
    pub battery_fg: Max17055<BatteryFgI2c>,

    pub misc_pins: MiscPins,
    pub high_prio_spawner: SendSpawner,
    pub wifi: WifiDriver,
}
