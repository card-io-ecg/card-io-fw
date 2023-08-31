use num_complex::Complex;

use crate::{filter::Filter, sliding::SlidingWindow, ComplExt};
use core::marker::PhantomData;

pub mod precomputed {
    use super::{HighPass, Iir};

    #[rustfmt::skip]
    pub const HIGH_PASS_FOR_DISPLAY_WEAK: Iir<'static, HighPass, 2> = macros::designfilt!(
        "highpassiir",
        "FilterOrder", 2,
        "HalfPowerFrequency", 0.5,
        "SampleRate", 1000
    );

    #[rustfmt::skip]
    pub const HIGH_PASS_FOR_DISPLAY_STRONG: Iir<'static, HighPass, 2> = macros::designfilt!(
        "highpassiir",
        "FilterOrder", 2,
        "HalfPowerFrequency", 1.5,
        "SampleRate", 1000
    );

    #[rustfmt::skip]
    pub const HIGH_PASS_50HZ: Iir<'static, HighPass, 2> = macros::designfilt!(
        "highpassiir",
        "FilterOrder", 2,
        "HalfPowerFrequency", 50,
        "SampleRate", 1000
    );

    #[rustfmt::skip]
    pub const HIGH_PASS_80HZ: Iir<'static, HighPass, 2> = macros::designfilt!(
        "highpassiir",
        "FilterOrder", 2,
        "HalfPowerFrequency", 80,
        "SampleRate", 1000
    );
}

pub struct HighPass;
pub struct LowPass;

pub trait FilterType {
    fn init_samples(sample: f32) -> (f32, f32);
}

impl FilterType for HighPass {
    fn init_samples(sample: f32) -> (f32, f32) {
        (sample, 0.0)
    }
}

impl FilterType for LowPass {
    fn init_samples(sample: f32) -> (f32, f32) {
        (sample, sample)
    }
}

pub struct Iir<'a, T, const N: usize>
where
    T: FilterType,
{
    previous_inputs: SlidingWindow<N>,
    previous_outputs: SlidingWindow<N>,

    num_coeffs: &'a [f32],
    denom_coeffs: &'a [f32],

    _marker: PhantomData<T>,
}

impl<'a, T, const N: usize> Iir<'a, T, N>
where
    T: FilterType,
{
    #[inline(always)]
    pub const fn new(num: &'a [f32], denom: &'a [f32]) -> Self {
        Self {
            previous_inputs: SlidingWindow::new(),
            previous_outputs: SlidingWindow::new(),
            num_coeffs: num,
            denom_coeffs: denom,
            _marker: PhantomData,
        }
    }

    pub fn transfer_coeff_at(&self, w: f32) -> Complex<f32> {
        let e_j_theta = |k: usize| Complex::from_polar(1.0, -1.0 * ((k + 1) as f32) * w);

        let mut num = Complex::new(self.num_coeffs[0], 0.0);
        let mut den = Complex::new(1.0, 0.0);

        for (k, coeff) in self.num_coeffs.iter().skip(1).enumerate() {
            num += coeff * e_j_theta(k + 1);
        }

        for (k, coeff) in self.denom_coeffs.iter().enumerate() {
            den += coeff * e_j_theta(k + 1);
        }

        num / den
    }
}

impl<T, const N: usize> Filter for Iir<'_, T, N>
where
    T: FilterType,
{
    fn update(&mut self, sample: f32) -> Option<f32> {
        if self.previous_inputs.is_empty() {
            for _ in 0..N {
                let (input, output) = T::init_samples(sample);
                self.previous_inputs.push(input);
                self.previous_outputs.push(output);
            }
            return None;
        }

        let mut y_out = sample * self.num_coeffs[0];

        for (coeff, spl) in self
            .previous_inputs
            .iter()
            .zip(self.num_coeffs.iter().skip(1).rev())
        {
            y_out += coeff * spl;
        }
        for (coeff, spl) in self
            .previous_outputs
            .iter()
            .zip(self.denom_coeffs.iter().rev())
        {
            y_out -= coeff * spl;
        }

        self.previous_inputs.push(sample);
        self.previous_outputs.push(y_out);

        Some(y_out)
    }

    fn clear(&mut self) {
        self.previous_inputs.clear();
        self.previous_outputs.clear();
    }
}
