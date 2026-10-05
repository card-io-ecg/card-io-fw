#![no_std]

// MUST be the first module
mod fmt;

pub mod data;
#[cfg(feature = "serve")]
mod handlers;

#[cfg(feature = "serve")]
pub use handlers::{ConfigSite, PairingControl, Station};
