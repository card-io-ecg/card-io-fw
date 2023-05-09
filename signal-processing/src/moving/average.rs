use crate::moving::sum::MovingSum;

#[derive(Default)]
pub struct MovingAverage<S: MovingSum> {
    sum: S,
}

impl<S: MovingSum> MovingAverage<S> {
    pub fn new() -> Self {
        Self { sum: S::new() }
    }

    pub fn clear(&mut self) {
        self.sum.clear();
    }

    pub fn update(&mut self, sample: f32) -> Option<f32> {
        self.sum.update(sample / (S::WINDOW_SIZE as f32))
    }
}
