use crate::filter::{
    iir::{Iir, LowPass},
    Filter,
};

pub struct DownsamplerLight {
    filter: Iir<'static, LowPass, 2>,
    counter: u8,
}

impl DownsamplerLight {
    pub const ECG_SR_1000HZ: DownsamplerLight = DownsamplerLight {
        #[rustfmt::skip]
        filter: macros::designfilt!(
            "lowpassiir",
            "FilterOrder", 2,
            "HalfPowerFrequency", 35,
            "SampleRate", 1000
        ),
        counter: 7,
    };
}

impl Filter for DownsamplerLight {
    fn clear(&mut self) {
        self.filter.clear();
        self.counter = 0;
    }

    fn update(&mut self, sample: f32) -> Option<f32> {
        let filtered = self.filter.update(sample)?;
        if self.counter == 0 {
            self.counter = 7;
            Some(filtered)
        } else {
            self.counter -= 1;
            None
        }
    }
}
