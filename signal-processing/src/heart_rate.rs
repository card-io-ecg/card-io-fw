use core::num::{NonZeroU32, NonZeroU8};

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

pub struct HeartRateCalculator<FMW, FB> {
    fs: SamplingFrequency,
    max_age: usize,
    max_init: usize,

    median: MedianFilter<3>,
    qrs_detector: QrsDetector<FMW, FB>,
    differentiator: SlidingWindow<2>,
    prev_detection: Option<NonZeroU32>,
    current_hr: Option<NonZeroU8>,
    is_beat: bool,
    age: usize,
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
            prev_detection: None,
            current_hr: None,
            is_beat: false,
            age: max_init,
        }
    }

    pub fn clear(&mut self) {
        self.median.clear();
        self.qrs_detector.clear();
        self.differentiator.clear();
        self.prev_detection = None;
        self.current_hr = None;
        self.is_beat = false;
        self.age = self.max_init;
    }

    pub fn update(&mut self, sample: f32) -> Option<f32> {
        let Some(old_sample) = self.differentiator.push(sample) else {
            return None;
        };

        let complex_lead = (sample - old_sample).abs();

        if let Some(idx) = self.qrs_detector.update(complex_lead) {
            if let Some(prev_idx) = self.prev_detection {
                let raw = self.fs.s_to_samples(60.0) as f32 / (idx - prev_idx.get()) as f32;
                let hr = self.median.update(raw).unwrap_or(raw);

                self.current_hr = NonZeroU8::new(hr as u8);
            }

            self.is_beat = true;
            self.prev_detection = NonZeroU32::new(idx);
            self.age = self.max_age;
        } else if self.age > 0 {
            self.is_beat = false;
            self.age -= 1;
        } else {
            self.clear();
        }

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
