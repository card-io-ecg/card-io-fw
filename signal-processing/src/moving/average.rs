use crate::moving::sum::MovingSum;

#[derive(Default, Clone)]
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
        let window_size = self.sum.window_size() as f32;
        self.sum.update(sample / window_size)
    }
}
