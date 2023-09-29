use core::ptr::NonNull;

use alloc::{boxed::Box, vec};

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
