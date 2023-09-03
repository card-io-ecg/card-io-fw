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

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_iir_no_input() {
        let input = [0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0.];
        let expectation = [0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0.];

        #[rustfmt::skip]
        test_filter(
            macros::designfilt!(
                "highpassiir",
                "FilterOrder", 1,
                "HalfPowerFrequency", 1,
                "SampleRate", 10
            ),
            &input,
            &expectation,
            0.0001
        );
    }

    #[test]
    fn test_iir_impluse_response_order1() {
        let input = [0., 1., 0., 0., 0., 0., 0., 0., 0., 0., 0.];
        let expectation = [
            0.7548, -0.3702, -0.1886, -0.0961, -0.0490, -0.0250, -0.0127, -0.0065, -0.0033, -0.0017,
        ];

        #[rustfmt::skip]
        test_filter(
            macros::designfilt!(
                "highpassiir",
                "FilterOrder", 1,
                "HalfPowerFrequency", 1,
                "SampleRate", 10
            ),
            &input,
            &expectation,
            0.0001
        );
    }

    #[test]
    fn test_iir_step_response_order1() {
        let input = [0., 1., 1., 1., 1., 1., 1., 1., 1., 1., 1.];
        let expectation = [
            0.7548, 0.3846, 0.1959, 0.0998, 0.0509, 0.0259, 0.0132, 0.0067, 0.0034, 0.0017,
        ];

        #[rustfmt::skip]
        test_filter(
            macros::designfilt!(
                "highpassiir",
                "FilterOrder", 1,
                "HalfPowerFrequency", 1,
                "SampleRate", 10
            ),
            &input,
            &expectation,
            0.0001
        );
    }

    #[test]
    fn test_iir_step_response_order2() {
        let input = [0., 1., 1., 1., 1., 1., 1., 1., 1., 1., 1.];
        let expectation = [
            0.6389, 0.0914, -0.1593, -0.2198, -0.1855, -0.1213, -0.0620, -0.0208, 0.0018, 0.0106,
        ];

        #[rustfmt::skip]
        test_filter(
            macros::designfilt!(
                "highpassiir",
                "FilterOrder", 2,
                "HalfPowerFrequency", 1,
                "SampleRate", 10
            ),
            &input,
            &expectation,
            0.0001
        );
    }

    #[test]
    fn test_iir_impluse_response_order2() {
        let input = [0., 1., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0.];
        let expectation = [
            0.6389, -0.5476, -0.2507, -0.0605, 0.0343, 0.0642, 0.0592, 0.0412, 0.0226, 0.0089,
        ];

        #[rustfmt::skip]
        test_filter(
            macros::designfilt!(
                "highpassiir",
                "FilterOrder", 2,
                "HalfPowerFrequency", 1,
                "SampleRate", 10
            ),
            &input,
            &expectation,
            0.0001
        );
    }

    #[track_caller]
    fn test_filter(mut filter: impl Filter, input: &[f32], expectation: &[f32], epsilon: f32) {
        let mut output = vec![];
        for sample in input.iter().copied() {
            if let Some(out_sample) = filter.update(sample) {
                output.push(out_sample);
            }
        }

        let pairs = output.iter().copied().zip(expectation.iter().copied());

        for (out_sample, expectation) in pairs.clone() {
            assert!(
                (out_sample - expectation).abs() < epsilon,
                "[\n  // (output, expectation)\n{}]",
                pairs
                    .map(|(a, b)| format!(
                        "   {} ({a:>7.04}, {b:>7.04})\n",
                        if (a - b).abs() < epsilon { " " } else { "!" }
                    ))
                    .collect::<String>()
            );
        }
    }
}
