//! Sliding window

use crate::buffer::Buffer;

pub struct SlidingWindow<const N: usize> {
    buffer: Buffer<f32, N>,
}

impl<const N: usize> Default for SlidingWindow<N> {
    #[inline(always)]
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl<const N: usize> SlidingWindow<N> {
    pub const DEFAULT: Self = Self::new();

    #[inline(always)]
    pub const fn new() -> Self {
        Self {
            buffer: Buffer::new(),
        }
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
    }

    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    pub fn is_full(&self) -> bool {
        self.buffer.is_full()
    }

    pub fn push(&mut self, sample: f32) -> Option<f32> {
        self.buffer.push(sample)
    }

    pub fn iter(&self) -> impl Iterator<Item = f32> + Clone + '_ {
        self.buffer.iter()
    }
}
