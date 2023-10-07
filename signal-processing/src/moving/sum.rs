use crate::sliding::SlidingWindow;

pub trait MovingSum {
    fn window_size(&self) -> usize;
    fn clear(&mut self);
    fn update(&mut self, sample: f32) -> Option<f32>;
}

#[derive(Default, Clone)]
pub struct Sum<const N: usize> {
    window: SlidingWindow<N>,
    current: f32,
}

impl<const N: usize> MovingSum for Sum<N> {
    #[inline(always)]
    fn window_size(&self) -> usize {
        N
    }

    fn clear(&mut self) {
        self.window.clear();
        self.current = 0.0;
    }

    fn update(&mut self, sample: f32) -> Option<f32> {
        self.current += sample;
        if let Some(old) = self.window.push(sample) {
            self.current -= old;
            Some(self.current)
        } else {
            None
        }
    }
}

#[derive(Default, Clone)]
pub struct EstimatedSum<const N: usize> {
    current: f32,
    samples: usize,
}

impl<const N: usize> MovingSum for EstimatedSum<N> {
    #[inline(always)]
    fn window_size(&self) -> usize {
        N
    }

    fn clear(&mut self) {
        self.current = 0.0;
        self.samples = 0;
    }

    fn update(&mut self, sample: f32) -> Option<f32> {
        if self.samples == N {
            self.current *= (N as f32 - 1.0) / (N as f32);
            self.current += sample;
            Some(self.current)
        } else {
            self.current += sample;
            self.samples += 1;
            None
        }
    }
}

#[cfg(feature = "alloc")]
use crate::sliding::AllocSlidingWindow;

#[cfg(feature = "alloc")]
#[derive(Clone)]
pub struct DynSum {
    window: AllocSlidingWindow,
    current: f32,
}

#[cfg(feature = "alloc")]
impl DynSum {
    #[inline(always)]
    pub fn new(window_size: usize) -> Self {
        Self {
            window: AllocSlidingWindow::new(window_size),
            current: 0.0,
        }
    }
}

#[cfg(feature = "alloc")]
impl MovingSum for DynSum {
    #[inline(always)]
    fn window_size(&self) -> usize {
        self.window.capacity()
    }

    fn clear(&mut self) {
        self.window.clear();
        self.current = 0.0;
    }

    fn update(&mut self, sample: f32) -> Option<f32> {
        self.current += sample;
        if let Some(old) = self.window.push(sample) {
            self.current -= old;
            Some(self.current)
        } else {
            None
        }
    }
}
