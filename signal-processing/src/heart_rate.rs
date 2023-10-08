use core::num::NonZeroU8;

use crate::{
    filter::{median::MedianFilter, Filter},
    sliding::SlidingWindow,
};
pub use qrs_detector::{
    sampling::{SamplingFrequency, SamplingFrequencyExt},
    QrsDetector, Thresholds,
};

#[allow(unused_imports)]
use crate::compat::*;

pub enum State {
    Init(usize),
    Measure(u32, usize),
}

pub struct HeartRateCalculator<FMW, FB> {
    fs: SamplingFrequency,
    max_age: usize,
    max_init: usize,

    median: MedianFilter<3>,
    qrs_detector: QrsDetector<FMW, FB>,
    differentiator: SlidingWindow<2>,
    state: State,
    current_hr: Option<NonZeroU8>,
    is_beat: bool,
}

impl HeartRateCalculator<(), ()> {
    #[inline]
    pub fn new<const SAMPLES_300: usize, const SAMPLES_50: usize>(
        fs: f32,
    ) -> HeartRateCalculator<[f32; SAMPLES_300], [f32; SAMPLES_50]> {
        let fs = fs.sps();

        HeartRateCalculator::new_from_qrs(fs, QrsDetector::new(fs))
    }

    #[cfg(feature = "alloc")]
    #[inline]
    pub fn new_alloc(
        fs: f32,
    ) -> HeartRateCalculator<alloc::boxed::Box<[f32]>, alloc::boxed::Box<[f32]>> {
        let fs = fs.sps();

        HeartRateCalculator::new_from_qrs(fs, QrsDetector::new_alloc(fs))
    }
}

impl<FMW, FB> HeartRateCalculator<FMW, FB>
where
    FMW: AsRef<[f32]> + AsMut<[f32]>,
    FB: AsRef<[f32]> + AsMut<[f32]>,
{
    fn new_from_qrs(fs: SamplingFrequency, qrs_detector: QrsDetector<FMW, FB>) -> Self {
        let max_init = fs.s_to_samples(5.0);
        let max_age = fs.s_to_samples(3.0);

        HeartRateCalculator {
            fs,
            max_age,
            max_init,

            median: MedianFilter::new(),
            qrs_detector,
            differentiator: SlidingWindow::new(),
            state: State::Init(max_init),
            current_hr: None,
            is_beat: false,
        }
    }

    pub fn clear(&mut self) {
        self.median.clear();
        self.qrs_detector.clear();
        self.differentiator.clear();
        self.state = State::Init(self.max_init);
        self.current_hr = None;
        self.is_beat = false;
    }

    pub fn update(&mut self, sample: f32) -> Option<f32> {
        let Some(old_sample) = self.differentiator.push(sample) else {
            return None;
        };

        let complex_lead = (sample - old_sample).abs();

        self.is_beat = false;
        self.state = match self.state {
            State::Init(0) => {
                self.clear();
                return Some(complex_lead);
            }
            State::Init(n) => {
                if let Some(idx) = self.qrs_detector.update(complex_lead) {
                    self.is_beat = true;
                    State::Measure(idx, self.max_age)
                } else {
                    State::Init(n - 1)
                }
            }

            State::Measure(prev_idx, age) => {
                if let Some(idx) = self.qrs_detector.update(complex_lead) {
                    let raw = self.fs.s_to_samples(60.0) as f32 / (idx - prev_idx) as f32;
                    let hr = self.median.update(raw).unwrap_or(raw);

                    self.current_hr = NonZeroU8::new(hr as u8);
                    self.is_beat = true;
                    State::Measure(idx, self.max_age)
                } else if age > 0 {
                    State::Measure(prev_idx, age - 1)
                } else {
                    self.clear();
                    return Some(complex_lead);
                }
            }
        };

        Some(complex_lead)
    }

    #[inline]
    pub fn thresholds(&self) -> Thresholds {
        self.qrs_detector.thresholds()
    }

    #[inline]
    pub fn current_hr(&self) -> Option<NonZeroU8> {
        self.current_hr
    }

    #[inline]
    pub fn is_beat(&self) -> bool {
        self.is_beat
    }
}
