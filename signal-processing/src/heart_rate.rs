use crate::filter::{fir::Fir, median::MedianFilter, Filter};
use qrs_detector::{sampling::SamplingFrequency, QrsDetector};

#[allow(clippy::excessive_precision)]
const LP_COEFFS: [f32; 113] = [
    0.000676934074286376,
    0.000480535317669767,
    0.000632224642324442,
    0.000799134028514435,
    0.000976481195817057,
    0.00115821572290706,
    0.00133658079650088,
    0.00150245914543360,
    0.00164531915017822,
    0.00175391246546628,
    0.00181645811863169,
    0.00182102633592723,
    0.00175615497843240,
    0.00161084591085913,
    0.00137548182790105,
    0.00104275669528121,
    0.000607931740628157,
    6.98508650888881e-05,
    -0.000569481722475432,
    -0.00130412583365705,
    -0.00212336937032632,
    -0.00301135383250486,
    -0.00394833067576165,
    -0.00491059210888043,
    -0.00586897687446980,
    -0.00678976765514748,
    -0.00763975758768483,
    -0.00837870755525578,
    -0.00896852805379440,
    -0.00936914182503847,
    -0.00954171502695708,
    -0.00944975791453415,
    -0.00905981887082908,
    -0.00834339079074862,
    -0.00727744530222625,
    -0.00584605253329541,
    -0.00404119804648787,
    -0.00186319243669471,
    0.000678183767102920,
    0.00356401083124424,
    0.00676575741277603,
    0.0102453586807236,
    0.0139565853066193,
    0.0178450696288120,
    0.0218494137549775,
    0.0259027986135662,
    0.0299346922599130,
    0.0338719114021147,
    0.0376404791345154,
    0.0411682296737852,
    0.0443860290572149,
    0.0472289897240658,
    0.0496400347727207,
    0.0515692365690961,
    0.0529767949018363,
    0.0538333254091582,
    0.0541208015561422,
    0.0538333254091582,
    0.0529767949018363,
    0.0515692365690961,
    0.0496400347727207,
    0.0472289897240658,
    0.0443860290572149,
    0.0411682296737852,
    0.0376404791345154,
    0.0338719114021147,
    0.0299346922599130,
    0.0259027986135662,
    0.0218494137549775,
    0.0178450696288120,
    0.0139565853066193,
    0.0102453586807236,
    0.00676575741277603,
    0.00356401083124424,
    0.000678183767102920,
    -0.00186319243669471,
    -0.00404119804648787,
    -0.00584605253329541,
    -0.00727744530222625,
    -0.00834339079074862,
    -0.00905981887082908,
    -0.00944975791453415,
    -0.00954171502695708,
    -0.00936914182503847,
    -0.00896852805379440,
    -0.00837870755525578,
    -0.00763975758768483,
    -0.00678976765514748,
    -0.00586897687446980,
    -0.00491059210888043,
    -0.00394833067576165,
    -0.00301135383250486,
    -0.00212336937032632,
    -0.00130412583365705,
    -0.000569481722475432,
    6.98508650888881e-05,
    0.000607931740628157,
    0.00104275669528121,
    0.00137548182790105,
    0.00161084591085913,
    0.00175615497843240,
    0.00182102633592723,
    0.00181645811863169,
    0.00175391246546628,
    0.00164531915017822,
    0.00150245914543360,
    0.00133658079650088,
    0.00115821572290706,
    0.000976481195817057,
    0.000799134028514435,
    0.000632224642324442,
    0.000480535317669767,
    0.000676934074286376,
];

pub enum State {
    Ignore(u32),
    Init(u32),
    Measure(u32, u32),
}

pub struct HeartRateCalculator {
    median: MedianFilter<3>,
    qrs_detector: QrsDetector<300, 50>,
    state: State,
    max_age: u32,
    max_init: u32,
    fs: f32,

    current_hr: Option<u8>,
    noise_filter: Fir<'static, 113>,
}

impl HeartRateCalculator {
    #[inline]
    pub fn new(fs: f32) -> Self {
        let max_init = (fs * 5.0) as u32;
        Self {
            median: MedianFilter::new(),
            qrs_detector: QrsDetector::new(SamplingFrequency(fs)),
            state: State::Ignore((0.5 * fs) as u32),
            max_age: (fs * 3.0) as u32,
            max_init,
            current_hr: None,
            noise_filter: Fir::from_coeffs(&LP_COEFFS),
            fs,
        }
    }

    pub fn clear(&mut self) {
        self.median.clear();
        self.qrs_detector.clear();
        self.state = State::Ignore((0.5 * self.fs) as u32);
        self.current_hr = None;
        self.noise_filter.clear();
    }

    pub fn update(&mut self, sample: f32) {
        let Some(sample) = self.noise_filter.update(sample) else {
            return;
        };

        self.state = match self.state {
            State::Ignore(0) => State::Init(self.max_init),
            State::Ignore(n) => State::Ignore(n - 1),

            State::Init(0) => {
                self.qrs_detector.clear();
                State::Init(self.max_init)
            }
            State::Init(n) => {
                if let Some(idx) = self.qrs_detector.update(sample) {
                    State::Measure(idx, self.max_age)
                } else {
                    State::Init(n - 1)
                }
            }

            State::Measure(prev_idx, age) => {
                if let Some(idx) = self.qrs_detector.update(sample) {
                    let raw = 60_000.0 / (idx - prev_idx) as f32;
                    let hr = self.median.update(raw).unwrap_or(raw);

                    self.current_hr = Some(hr as u8);

                    State::Measure(idx, self.max_age)
                } else if age > 0 {
                    State::Measure(prev_idx, age - 1)
                } else {
                    self.median.clear();
                    self.qrs_detector.clear();
                    self.noise_filter.clear();
                    self.current_hr = None;

                    State::Init(self.max_init)
                }
            }
        };
    }

    #[inline]
    pub fn current_hr(&self) -> Option<u8> {
        self.current_hr
    }
}
