use crate::moving::sum::MovingSum;

pub trait MovingVariance {
    const WINDOW_SIZE: usize;

    fn new() -> Self;
    fn clear(&mut self);
    fn update(&mut self, sample: f32) -> Option<f32>;
}

pub struct MovingVarianceOfErgodic<S: MovingSum> {
    sum: S,
}

impl<S: MovingSum> MovingVariance for MovingVarianceOfErgodic<S> {
    const WINDOW_SIZE: usize = S::WINDOW_SIZE;

    #[inline(always)]
    fn new() -> Self {
        Self { sum: S::new() }
    }

    fn clear(&mut self) {
        self.sum.clear();
    }

    fn update(&mut self, sample: f32) -> Option<f32> {
        self.sum.update(sample * sample / (S::WINDOW_SIZE as f32))
    }
}
