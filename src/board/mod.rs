#[cfg_attr(feature = "hw_v1", path = "hardware/v1.rs")]
#[cfg_attr(feature = "hw_v2", path = "hardware/v2.rs")]
pub mod hardware;

pub mod config;
pub mod drivers;
pub mod initialized;
pub mod startup;
pub mod utils;
pub mod wifi;

use esp_backtrace as _;

#[cfg(feature = "esp32s2")]
pub use esp32s2_hal as hal;

#[cfg(feature = "esp32s3")]
pub use esp32s3_hal as hal;

#[cfg(feature = "esp32s2")]
pub use esp32s2 as pac;

#[cfg(feature = "esp32s3")]
pub use esp32s3 as pac;

pub use hardware::*;

use signal_processing::battery::BatteryModel;

pub struct MiscPins {
    pub vbus_detect: VbusDetect,
    pub chg_status: ChargerStatus,
}

pub const BATTERY_MODEL: BatteryModel = BatteryModel {
    voltage: (2750, 4200),
    charge_current: (0, 1000),
};

pub const LOW_BATTERY_VOLTAGE: u16 = 3300;
