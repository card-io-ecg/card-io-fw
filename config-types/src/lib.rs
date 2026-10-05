#![no_std]

// MUST be the first module
mod fmt;

pub const LOW_BATTERY_PERCENTAGE: u8 = 5;

pub mod current;
pub mod measurement_queue;
pub mod record;
pub mod types;

pub use current::Config;
