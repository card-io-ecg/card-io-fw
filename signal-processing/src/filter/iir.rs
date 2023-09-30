use num_complex::Complex;

use crate::{filter::Filter, sliding::SlidingWindow};

#[allow(unused_imports)]
use crate::compat::*;

pub mod precomputed {
    use super::{HighPass, Iir};

    pub const ALL_PASS: Iir<'static, HighPass, 2> = Iir::new(&[1.], &[]);
}

pub trait IirFilter {
    fn transfer_coeff_at(&self, w: f32) -> Complex<f32>;
}

#[derive(Clone)]
pub struct HighPass {
    first_sample: Option<f32>,
}

#[derive(Clone)]
pub struct LowPass;

pub trait FilterType {
    const NEW: Self;

    fn clear(&mut self) {}
    fn precondition(&mut self, sample: f32) -> f32;
}

impl FilterType for HighPass {
    const NEW: Self = Self { first_sample: None };

    fn clear(&mut self) {
        self.first_sample = None;
    }

    fn precondition(&mut self, sample: f32) -> f32 {
        let first_sample = self.first_sample.get_or_insert(sample);
        sample - *first_sample
    }
}

impl FilterType for LowPass {
    const NEW: Self = Self;

    fn precondition(&mut self, sample: f32) -> f32 {
        sample
    }
}

#[derive(Clone)]
pub struct Iir<'a, T, const N: usize> {
    previous_inputs: SlidingWindow<N>,
    previous_outputs: SlidingWindow<N>,

    num_coeffs: &'a [f32],
    denom_coeffs: &'a [f32],

    filter_kind: T,
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
            filter_kind: T::NEW,
        }
    }
}

impl<'a, T, const N: usize> IirFilter for Iir<'a, T, N> {
    fn transfer_coeff_at(&self, w: f32) -> Complex<f32> {
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
        let sample = self.filter_kind.precondition(sample);

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
        self.filter_kind.clear();
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
    fn test_lowpass_iir_impluse_response_order1() {
        let input = [0., 1., 0., 0., 0., 0., 0.];
        let expectation = [0.0000, 0.2452, 0.3702, 0.1886, 0.0961, 0.0490, 0.0250];

        #[rustfmt::skip]
        test_filter(
            macros::designfilt!(
                "lowpassiir",
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
    fn test_lowpass_iir_step_response_order1() {
        let input = [0., 1., 1., 1., 1., 1., 1.];
        let expectation = [0.0000, 0.2452, 0.6154, 0.8041, 0.9002, 0.9491, 0.9741];

        #[rustfmt::skip]
        test_filter(
            macros::designfilt!(
                "lowpassiir",
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
    fn test_highpass_iir_impluse_response_order1() {
        let input = [0., 1., 0., 0., 0., 0., 0.];
        let expectation = [0.0000, 0.7548, -0.3702, -0.1886, -0.0961, -0.0490, -0.0250];

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
    fn test_highpass_iir_step_response_order1() {
        let input = [0., 1., 1., 1., 1., 1., 1.];
        let expectation = [0.0000, 0.7548, 0.3846, 0.1959, 0.0998, 0.0509, 0.0259];

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
    fn test_lowpass_iir_impluse_response_order2() {
        let input = [0., 1., 0., 0., 0., 0., 0., 0.];
        let expectation = [0.0000, 0.0675, 0.2120, 0.2819, 0.2347, 0.1519, 0.0767];

        #[rustfmt::skip]
        test_filter(
            macros::designfilt!(
                "lowpassiir",
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
    fn test_lowpass_iir_step_response_order2() {
        let input = [0., 1., 1., 1., 1., 1., 1.];
        let expectation = [0.0000, 0.0675, 0.2795, 0.5614, 0.7961, 0.9480, 1.0248];

        #[rustfmt::skip]
        test_filter(
            macros::designfilt!(
                "lowpassiir",
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
    fn test_highpass_iir_impluse_response_order2() {
        let input = [0., 1., 0., 0., 0., 0., 0., 0.];
        let expectation = [0.0000, 0.6389, -0.5476, -0.2507, -0.0605, 0.0343, 0.0642];

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
    fn test_highpass_iir_step_response_order2() {
        let input = [0., 1., 1., 1., 1., 1., 1.];
        let expectation = [0.0000, 0.6389, 0.0914, -0.1593, -0.2198, -0.1855, -0.1213];

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
