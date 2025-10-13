#![no_std]
#![allow(stable_features)]
#![feature(async_fn_in_trait)]
#![feature(generic_const_exprs)] // norfs needs this
#![allow(unknown_lints, async_fn_in_trait)]
#![allow(incomplete_features)] // generic_const_exprs

extern crate alloc;

pub use embedded_layout;

// MUST be the first module
mod fmt;

pub mod screens;
pub mod utils;
pub mod widgets;
