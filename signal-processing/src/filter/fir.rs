use super::Filter;

pub struct Fir<'a, const N: usize> {
    coeffs: &'a [f32; N],
    buffer: [f32; N],
    idx: usize,
    full: bool,
}

impl<'a, const N: usize> Fir<'a, N> {
    pub fn from_coeffs(coeffs: &'a [f32; N]) -> Self {
        Self {
            coeffs,
            buffer: [0.0; N],
            idx: 0,
            full: false,
        }
    }

    fn push(&mut self, sample: f32) {
        self.buffer[self.idx] = sample;
        self.idx = (self.idx + 1) % self.buffer.len();
        if self.idx == 0 {
            self.full = true;
        }
    }

    fn iter_points(&self) -> impl Iterator<Item = f32> + '_ {
        (self.idx..self.buffer.len())
            .chain(0..self.idx)
            .map(|i| self.buffer[i])
    }
}

impl<'a, const N: usize> Filter for Fir<'a, N> {
    fn clear(&mut self) {
        self.idx = 0;
        self.full = false;
    }

    fn update(&mut self, sample: f32) -> Option<f32> {
        self.push(sample);

        self.full.then(|| {
            self.iter_points()
                .zip(self.coeffs.iter())
                .map(|(a, b)| a * b)
                .sum()
        })
    }
}
