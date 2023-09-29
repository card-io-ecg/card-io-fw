//! Power-line interference suppression filter
//!
//! Algorithm detailed in <https://pdfs.semanticscholar.org/64e4/187ffd6c72e44e849df203e0401f52eb0a27.pdf?_ga=1.255800139.271070806.1489245978>
//!
//! Implementation loosely based on matlab code found in <https://github.com/s-gv/rnicu/blob/master/ecg/adaptive_filter/pll_martens_errorfilt_supp.m>

use crate::filter::{
    iir::{HighPass, Iir, IirFilter},
    Filter,
};

#[allow(unused_imports)]
use crate::compat::*;

#[derive(Clone)]
struct FilterCore<F> {
    // constants
    frequency: f32,
    gamma: f32, // error filter attenuation

    phase_filter: F,
    amplitude_filter: F,

    // amplitude correction factor
    alpha: f32,

    // estimated signal parameters
    theta_phi: f32,
    theta_a: f32,
    theta_dw: f32,

    // signatures
    y_mod_a: f32,
    y_mod_phi: f32,
}

impl<F> FilterCore<F>
where
    F: Filter + IirFilter + Clone,
{
    #[inline(always)]
    fn new(fs: f32, frequency: f32, sig_filter: F) -> Self {
        let frequency = 2.0 * core::f32::consts::PI * frequency / fs;

        Self {
            frequency,
            gamma: 2.0 / sig_filter.transfer_coeff_at(frequency).norm(),

            phase_filter: sig_filter.clone(),
            amplitude_filter: sig_filter,

            alpha: 1.0,

            theta_phi: 0.0,
            theta_a: 0.0,
            theta_dw: 0.0,

            y_mod_a: 0.0,
            y_mod_phi: 0.0,
        }
    }
}

impl<F> FilterCore<F>
where
    F: Filter,
{
    fn clear(&mut self) {
        self.phase_filter.clear();
        self.amplitude_filter.clear();

        self.alpha = 1.0;
        self.theta_phi = 0.0;
        self.theta_a = 0.0;
        self.theta_dw = 0.0;
        self.y_mod_a = 0.0;
        self.y_mod_phi = 0.0;
    }

    fn estimate(&mut self, idx: usize) -> f32 {
        let t = self.frequency * (idx as f32) + self.theta_phi;

        let osc_i = t.sin();
        let osc_q = self.alpha * t.cos();

        // preserve defaults initially
        self.y_mod_a = self.amplitude_filter.update(osc_i).unwrap_or(self.y_mod_a);
        self.y_mod_phi = self.phase_filter.update(osc_q).unwrap_or(self.y_mod_phi);

        // always update the estimated phase based on the
        // frequency deviation, even when adaptation is blocked
        self.theta_phi += self.theta_dw;

        self.theta_a * osc_i
    }

    fn adapt(&mut self, filter: &Constants, ew: f32) {
        /* 2.0 multiplier merged into gamma */
        let eta_phi = self.gamma * ew * self.y_mod_phi;
        let eta_a = self.gamma * ew * self.y_mod_a;

        let thetaa_est_new = self.theta_a + filter.k_a * eta_a;
        let thetadw_est_new = self.theta_dw + Constants::K_DW * eta_phi;
        // not a bug: theta_dw added to theta_phi in estimate
        let thetaphi_est_new = self.theta_phi + Constants::K_PHI * eta_phi;

        if thetaa_est_new > 0.0 {
            self.theta_a = thetaa_est_new;
            self.alpha = 1.0 / thetaa_est_new;
        }
        if thetaphi_est_new.abs() < filter.theta_dw_update_threshold {
            self.theta_dw = thetadw_est_new;
        }
        self.theta_phi = thetaphi_est_new;
    }
}

pub mod adaptation_blocking {
    use crate::{
        filter::{comb::CombFilter, Filter},
        moving::{
            sum::MovingSum,
            variance::{MovingVariance, MovingVarianceOfErgodic},
        },
        sliding::SlidingWindow,
    };

    #[allow(unused_imports)]
    use crate::compat::*;

    pub trait AdaptationBlockingTrait: Default {
        fn update(&mut self, sample: f32) -> Option<(f32, bool)>;
        fn clear(&mut self);
    }

    #[derive(Default, Clone)]
    pub struct NoAdaptationBlocking;

    #[derive(Default, Clone)]
    pub struct AdaptationBlocking<V, const L: usize, const C: usize>
    where
        V: MovingSum,
    {
        delay: SlidingWindow<L>,
        comb_filter: CombFilter<C>,
        variance: MovingVarianceOfErgodic<V>,
        delay_cnt: usize,
    }

    impl AdaptationBlockingTrait for NoAdaptationBlocking {
        fn update(&mut self, sample: f32) -> Option<(f32, bool)> {
            Some((sample, false))
        }
        fn clear(&mut self) {}
    }

    impl<V, const L: usize, const C: usize> AdaptationBlockingTrait for AdaptationBlocking<V, L, C>
    where
        V: MovingSum + Default,
    {
        fn update(&mut self, sample: f32) -> Option<(f32, bool)> {
            let delayed_sample = self.delay.push(sample);
            let comb_filtered = self.comb_filter.update(sample)?;
            let variance = self.variance.update(comb_filtered)?;

            self.delay_cnt = if comb_filtered.abs() > (2.0 * variance).sqrt() {
                2 * L
            } else {
                self.delay_cnt.saturating_sub(1)
            };

            delayed_sample.map(|delayed_sample| (delayed_sample, self.delay_cnt > 0))
        }

        fn clear(&mut self) {
            self.comb_filter.clear();
            self.delay.clear();
            self.delay_cnt = 0;
            self.variance.clear();
        }
    }
}

#[derive(Clone)]
pub struct Constants {
    k_a: f32,
    theta_dw_update_threshold: f32,
}

impl Constants {
    const K_A: f32 = 1.0 / 0.13;
    const K_PHI: f32 = 6e-2;
    const K_DW: f32 = 9e-4;

    #[inline(always)]
    fn new(fs: f32) -> Self {
        Self {
            k_a: Self::K_A / fs,
            theta_dw_update_threshold: 4.0 / fs,
        }
    }
}

#[derive(Clone)]
pub struct PowerLineFilter<ADB, F, const N_FS: usize>
where
    ADB: adaptation_blocking::AdaptationBlockingTrait,
{
    // configuration
    consts: Constants,
    cores: [FilterCore<F>; N_FS],
    adaptation_blocking: ADB,
    error_filter: F,
    sample_idx: usize,
}

impl<ADB, const N_FS: usize> PowerLineFilter<ADB, Iir<'static, HighPass, 2>, N_FS>
where
    ADB: adaptation_blocking::AdaptationBlockingTrait,
{
    #[inline]
    pub fn new_1ksps(frequencies: [f32; N_FS]) -> Self {
        #[rustfmt::skip]
        const SIGNATURE_FILTER: Iir<HighPass, 2> = macros::designfilt!(
            "highpassiir",
            "FilterOrder", 2,
            "HalfPowerFrequency", 50,
            "SampleRate", 1000
        );
        #[rustfmt::skip]
        const ERROR_FILTER: Iir<HighPass, 2> = macros::designfilt!(
            "highpassiir",
            "FilterOrder", 2,
            "HalfPowerFrequency", 50,
            "SampleRate", 1000
        );
        Self {
            consts: Constants::new(1000.0),
            cores: frequencies.map(|f| FilterCore::new(1000.0, f, SIGNATURE_FILTER)),
            adaptation_blocking: ADB::default(),
            error_filter: ERROR_FILTER,
            sample_idx: 0,
        }
    }
}

#[cfg(feature = "dyn_filter")]
use crate::filter::dyn_iir::DynIir;

#[cfg(feature = "dyn_filter")]
impl<ADB, const N_FS: usize> PowerLineFilter<ADB, DynIir<HighPass, 2>, N_FS>
where
    ADB: adaptation_blocking::AdaptationBlockingTrait,
{
    #[inline]
    pub fn design(fs: f32, frequencies: [f32; N_FS]) -> Self {
        Self {
            consts: Constants::new(fs),
            cores: frequencies.map(|f| FilterCore::new(fs, f, DynIir::design(fs, f))),
            adaptation_blocking: ADB::default(),
            error_filter: DynIir::design(fs, 50.0),
            sample_idx: 0,
        }
    }
}

impl<ADB, F, const N_FS: usize> Filter for PowerLineFilter<ADB, F, N_FS>
where
    ADB: adaptation_blocking::AdaptationBlockingTrait,
    F: Filter,
{
    fn clear(&mut self) {
        self.cores.iter_mut().for_each(FilterCore::clear);
        self.sample_idx = 0;
        self.error_filter.clear();
        self.adaptation_blocking.clear();
    }

    fn update(&mut self, sample: f32) -> Option<f32> {
        let (delayed_sample, adapt_blocked) = self.adaptation_blocking.update(sample)?;

        let x_est = self
            .cores
            .iter_mut()
            .map(|core| core.estimate(self.sample_idx))
            .sum::<f32>();

        self.sample_idx += 1;

        let error = delayed_sample - x_est;
        let filtered_error = self.error_filter.update(error)?;

        if !adapt_blocked {
            self.cores
                .iter_mut()
                .for_each(|core| core.adapt(&self.consts, filtered_error));
        }

        Some(error)
    }
}
