#[cfg_attr(feature = "hw_v4", path = "hardware/v4.rs")]
#[cfg_attr(all(feature = "hw_v6", feature = "esp32s3"), path = "hardware/v6s3.rs")]
#[cfg_attr(all(feature = "hw_v6", feature = "esp32c6"), path = "hardware/v6c6.rs")]
#[cfg_attr(all(feature = "hw_v8", feature = "esp32s3"), path = "hardware/v8s3.rs")]
#[cfg_attr(all(feature = "hw_v8", feature = "esp32c6"), path = "hardware/v8c6.rs")]
#[cfg_attr( // We default to hw_v8/esp32c6 if no feature is selected to help rust-analyzer for example
    not(any(
        feature = "hw_v4",
        all(feature = "hw_v6", feature = "esp32s3"),
        all(feature = "hw_v6", feature = "esp32c6"),
        all(feature = "hw_v8", feature = "esp32s3"),
        all(feature = "hw_v8", feature = "esp32c6"),
    )),
    path = "hardware/v8c6.rs"
)]
pub mod hardware;

pub mod drivers;
pub mod initialized;
#[cfg(feature = "wifi")]
pub mod ota;
pub mod startup;
pub mod storage;
pub mod utils;
#[cfg(feature = "wifi")]
pub mod wifi;

#[cfg(feature = "esp-println")]
use esp_backtrace as _;
#[cfg(feature = "rtt")]
use panic_rtt_target as _;

pub use hardware::*;
