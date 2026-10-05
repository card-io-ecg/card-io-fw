#![no_std]
#![feature(allocator_api)] // Box::try_new

extern crate alloc;

#[cfg(test)]
extern crate std;

// MUST be the first module
mod fmt;

pub mod client;
pub mod http;
pub mod pairing;
pub mod url;
