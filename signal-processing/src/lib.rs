#![cfg_attr(not(test), no_std)]

#[cfg(feature = "alloc")]
extern crate alloc;

// MUST be the first module
mod fmt;

pub mod battery;
pub mod buffer;
pub mod compressing_buffer;
pub mod filter;
pub mod heart_rate;
pub mod lerp;
pub mod moving;
pub mod sliding;

mod compat {
    pub use micromath::F32Ext;

    #[cfg(not(feature = "std"))]
    use num_complex::Complex;

    #[cfg(not(feature = "std"))]
    pub trait ComplExt {
        fn from_polar(mag: f32, phase: f32) -> Complex<f32>;
        fn norm(&self) -> f32;
    }

    #[cfg(not(feature = "std"))]
    impl ComplExt for Complex<f32> {
        fn from_polar(mag: f32, phase: f32) -> Complex<f32> {
            mag * Complex::new(phase.cos(), phase.sin())
        }

        fn norm(&self) -> f32 {
            self.norm_sqr().sqrt()
        }
    }
}
