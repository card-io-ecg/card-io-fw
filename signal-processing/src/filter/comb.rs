use crate::{filter::Filter, sliding::SlidingWindow};

#[derive(Default, Clone)]
pub struct CombFilter<const N: usize> {
    window: SlidingWindow<N>,
}

impl<const N: usize> CombFilter<N> {
    pub const DEFAULT: Self = Self {
        window: SlidingWindow::new(),
    };

    #[inline(always)]
    pub const fn new() -> Self {
        Self::DEFAULT
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
