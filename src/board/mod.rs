#[cfg_attr(feature = "hw_v1", path = "hardware/v1.rs")]
#[cfg_attr(feature = "hw_v2", path = "hardware/v2.rs")]
#[cfg_attr(feature = "hw_v4", path = "hardware/v4.rs")]
#[cfg_attr(all(feature = "hw_v6", feature = "esp32s3"), path = "hardware/v6s3.rs")]
#[cfg_attr(all(feature = "hw_v6", feature = "esp32c6"), path = "hardware/v6c6.rs")]
#[cfg_attr( // We default to hw_v6/esp32c6 if no feature is selected to help rust-analyzer for example
    not(any(
        feature = "hw_v1",
        feature = "hw_v2",
        feature = "hw_v4",
        all(feature = "hw_v6", feature = "esp32s3"),
        all(feature = "hw_v6", feature = "esp32c6")
    )),
    path = "hardware/v6c6.rs"
)]
pub mod hardware;

pub mod config;
pub mod drivers;
pub mod initialized;
pub mod ota;
pub mod startup;
pub mod storage;
pub mod utils;
pub mod wifi;

use esp_backtrace as _;

#[cfg(feature = "esp32s2")]
pub use esp32s2_hal as hal;

#[cfg(feature = "esp32s3")]
pub use esp32s3_hal as hal;

#[cfg(feature = "esp32c6")]
pub use esp32c6_hal as hal;

pub use hardware::*;

pub const DEFAULT_BACKEND_URL: &str = "https://stingray-prime-monkey.ngrok-free.app";
pub const LOW_BATTERY_PERCENTAGE: u8 = 5;

#[cfg(feature = "esp32c6")]
#[no_mangle]
extern "C" fn fminf(x: f32, y: f32) -> f32 {
    if x < y {
        x
    } else {
        y
    }
}

#[cfg(feature = "esp32c6")]
#[no_mangle]
extern "C" fn fmaxf(x: f32, y: f32) -> f32 {
    if x > y {
        x
    } else {
        y
    }
}
