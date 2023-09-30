use core::ptr::NonNull;

use alloc::{boxed::Box, vec};

use num_complex::Complex;
use sci_rs::signal::filter::design::{
    iirfilter_dyn, BaFormatFilter, DigitalFilter, FilterBandType, FilterOutputType,
    FilterType as DesignFilterType,
};

use crate::filter::Filter;

pub use super::iir::*;

pub trait DynFilterType: FilterType {
    const BAND_TYPE: FilterBandType;
}

impl DynFilterType for HighPass {
    const BAND_TYPE: FilterBandType = FilterBandType::Highpass;
}

impl DynFilterType for LowPass {
    const BAND_TYPE: FilterBandType = FilterBandType::Lowpass;
}

#[derive(Clone)]
pub struct DynIir<T, const N: usize>
where
    T: FilterType,
{
    _num_coeffs: Box<[f32]>,
    _denom_coeffs: Box<[f32]>,

    filter: Iir<'static, T, N>,
}

impl<T, const N: usize> DynIir<T, N>
where
    T: DynFilterType,
{
    pub fn design(fs: f32, f_half_power: f32) -> Self {
        let filter = iirfilter_dyn(
            N,
            vec![f_half_power],
            None,
            None,
            Some(T::BAND_TYPE),
            Some(DesignFilterType::Butterworth),
            Some(false),
            Some(FilterOutputType::Ba),
            Some(fs),
        );

        let DigitalFilter::Ba(BaFormatFilter { mut b, mut a }) = filter else {
            unreachable!()
        };

        // count trailing zeros
        let zeros_a = a.iter().rev().take_while(|&&x| x == 0.0).count();
        let zeros_b = b.iter().rev().take_while(|&&x| x == 0.0).count();

        let remove = zeros_a.min(zeros_b);

        a.truncate(a.len() - remove);
        b.truncate(b.len() - remove);

        // Strip off always-1 coefficient
        assert!(a.remove(0) == 1.0);

        // we reverse a to avoid having to reverse it during filtering
        a.reverse();

        // b seems to be returned in the wrong order
        b.reverse();

        let denom_coeffs = a.into_boxed_slice();
        let num_coeffs = b.into_boxed_slice();

        let a = unsafe { NonNull::from(denom_coeffs.as_ref()).as_ref() };
        let b = unsafe { NonNull::from(num_coeffs.as_ref()).as_ref() };

        Self {
            _num_coeffs: num_coeffs,
            _denom_coeffs: denom_coeffs,
            filter: Iir::new(&b, &a),
        }
    }
}

impl<T: DynFilterType, const N: usize> Filter for DynIir<T, N> {
    fn update(&mut self, sample: f32) -> Option<f32> {
        self.filter.update(sample)
    }

    fn clear(&mut self) {
        self.filter.clear()
    }
}

impl<T: DynFilterType, const N: usize> IirFilter for DynIir<T, N> {
    fn transfer_coeff_at(&self, w: f32) -> Complex<f32> {
        self.filter.transfer_coeff_at(w)
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
            DynIir::<HighPass, 1>::design(10.0, 1.0),
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
            DynIir::<LowPass, 1>::design(10.0, 1.0),
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
            DynIir::<LowPass, 1>::design(10.0, 1.0),
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
            DynIir::<HighPass, 1>::design(10.0, 1.0),
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
            DynIir::<HighPass, 1>::design(10.0, 1.0),
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
            DynIir::<LowPass, 2>::design(10.0, 1.0),
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
            DynIir::<LowPass, 2>::design(10.0, 1.0),
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
            DynIir::<HighPass, 2>::design(10.0, 1.0),
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
            DynIir::<HighPass, 2>::design(10.0, 1.0),
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
