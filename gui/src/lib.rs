#![no_std]
#![allow(stable_features)]
#![feature(async_fn_in_trait)]
#![allow(unknown_lints, async_fn_in_trait)]

extern crate alloc;

pub use embedded_layout;

// MUST be the first module
mod fmt;

pub mod screens;
pub mod utils;
pub mod widgets;
