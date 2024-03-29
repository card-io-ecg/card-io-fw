use crate::sliding::SlidingWindow;

use super::Filter;

#[derive(Clone)]
pub struct Fir<'a, const N: usize> {
    coeffs: &'a [f32; N],
    buffer: SlidingWindow<N>,
}

impl<'a, const N: usize> Fir<'a, N> {
    #[inline(always)]
    pub const fn from_coeffs(coeffs: &'a [f32; N]) -> Self {
        Self {
            coeffs,
            buffer: SlidingWindow::new(),
        }
    }
}

impl<'a, const N: usize> Filter for Fir<'a, N> {
    fn clear(&mut self) {
        self.buffer.clear()
    }

    fn update(&mut self, sample: f32) -> Option<f32> {
        self.buffer.push(sample);

        self.buffer.is_full().then(|| {
            self.buffer
                .iter()
                .zip(self.coeffs.iter())
                .map(|(a, b)| a * b)
                .sum()
        })
    }
}
