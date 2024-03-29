use crate::moving::sum::MovingSum;

pub trait MovingVariance {
    fn window_size(&self) -> usize;
    fn clear(&mut self);
    fn update(&mut self, sample: f32) -> Option<f32>;
}

#[derive(Default, Clone)]
pub struct MovingVarianceOfErgodic<S: MovingSum> {
    sum: S,
}

impl<S: MovingSum> MovingVarianceOfErgodic<S> {
    #[inline(always)]
    pub fn new(sum: S) -> Self {
        Self { sum }
    }
}

impl<S: MovingSum> MovingVariance for MovingVarianceOfErgodic<S> {
    #[inline(always)]
    fn window_size(&self) -> usize {
        self.sum.window_size()
    }

    fn clear(&mut self) {
        self.sum.clear();
    }

    fn update(&mut self, sample: f32) -> Option<f32> {
        let window_size = self.sum.window_size() as f32;
        self.sum.update(sample * sample / window_size)
    }
}
