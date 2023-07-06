#![cfg_attr(feature = "nostd", no_std)]

pub mod battery;
pub mod buffer;
pub mod compressing_buffer;
pub mod filter;
pub mod heart_rate;
pub mod i24;
pub mod lerp;
pub mod moving;
pub mod sliding;

#[cfg(feature = "nostd")]
use micromath::F32Ext;
use num_complex::Complex;

pub trait ComplExt {
    fn from_polar(mag: f32, phase: f32) -> Complex<f32>;
    fn norm(&self) -> f32;
}

impl ComplExt for Complex<f32> {
    fn from_polar(mag: f32, phase: f32) -> Complex<f32> {
        mag * Complex::new(phase.cos(), phase.sin())
    }

    fn norm(&self) -> f32 {
        self.norm_sqr().sqrt()
    }
}
