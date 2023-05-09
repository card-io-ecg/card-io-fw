use crate::sliding::SlidingWindow;

#[derive(Default)]
pub struct Sum<const N: usize> {
    window: SlidingWindow<N>,
    current: f32,
}

pub trait MovingSum {
    const WINDOW_SIZE: usize;

    fn new() -> Self;
    fn clear(&mut self);
    fn update(&mut self, sample: f32) -> Option<f32>;
}

impl<const N: usize> MovingSum for Sum<N> {
    const WINDOW_SIZE: usize = N;

    fn new() -> Self {
        Self {
            window: SlidingWindow::new(),
            current: 0.0,
        }
    }

    fn clear(&mut self) {
        *self = Self::new();
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

#[derive(Default)]
pub struct EstimatedSum<const N: usize> {
    current: f32,
    samples: usize,
}

impl<const N: usize> MovingSum for EstimatedSum<N> {
    const WINDOW_SIZE: usize = N;

    fn new() -> Self {
        Self {
            current: 0.0,
            samples: 0,
        }
    }

    fn clear(&mut self) {
        *self = Self::new();
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
