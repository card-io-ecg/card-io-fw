use super::{fir::Fir, Filter};

const COEFFS: &[f32; 43] = &[
    0.001_896_49,
    0.001_427_698_3,
    -0.005_418_506_5,
    -0.014_846_274,
    -0.014_311_559_5,
    0.000_110_694_32,
    0.011_810_528,
    0.003_197_918,
    -0.014_473_969,
    -0.011_425_788_5,
    0.013_566_333,
    0.021_809_606,
    -0.007_811_884_4,
    -0.033_718_437,
    -0.005_609_694_4,
    0.045_531_593,
    0.031_312_115,
    -0.055_580_772,
    -0.084_990_874,
    0.062_316_805,
    0.310_974_66,
    0.435_316_83,
    0.310_974_66,
    0.062_316_805,
    -0.084_990_874,
    -0.055_580_772,
    0.031_312_115,
    0.045_531_593,
    -0.005_609_694_4,
    -0.033_718_437,
    -0.007_811_884_4,
    0.021_809_606,
    0.013_566_333,
    -0.011_425_788_5,
    -0.014_473_969,
    0.003_197_918,
    0.011_810_528,
    0.000_110_694_32,
    -0.014_311_559_5,
    -0.014_846_274,
    -0.005_418_506_5,
    0.001_427_698_3,
    0.001_896_49,
];

pub struct DownSampler {
    filter: Fir<'static, 43>,
    output_next: bool,
}

impl DownSampler {
    pub const DEFAULT: Self = Self {
        filter: Fir::from_coeffs(COEFFS),
        output_next: false,
    };

    #[inline(always)]
    pub const fn new() -> Self {
        Self::DEFAULT
    }
}

impl Default for DownSampler {
    #[inline(always)]
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl Filter for DownSampler {
    #[inline]
    fn clear(&mut self) {
        self.filter.clear();
        self.output_next = false;
    }

    #[inline]
    fn update(&mut self, sample: f32) -> Option<f32> {
        let filtered = self.filter.update(sample)?;

        let output = self.output_next;
        self.output_next = !output;

        if output {
            Some(filtered)
        } else {
            None
        }
    }
}
