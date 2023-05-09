use crate::{filter::Filter, sliding::SlidingWindow};

#[derive(Default)]
pub struct CombFilter<const N: usize> {
    window: SlidingWindow<N>,
}

impl<const N: usize> CombFilter<N> {
    pub fn new() -> Self {
        Self {
            window: SlidingWindow::new(),
        }
    }
}

impl<const N: usize> Filter for CombFilter<N> {
    fn update(&mut self, sample: f32) -> Option<f32> {
        self.window.push(sample).map(|old| sample - old)
    }

    fn clear(&mut self) {
        self.window.clear();
    }
}
