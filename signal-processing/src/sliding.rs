//! Sliding window

use crate::buffer::Buffer;

#[derive(Clone)]
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

#[cfg(feature = "alloc")]
#[derive(Clone)]
pub struct AllocSlidingWindow {
    buffer: alloc::boxed::Box<[f32]>,
    write_idx: usize,
    count: usize,
}

#[cfg(feature = "alloc")]
impl AllocSlidingWindow {
    #[inline(always)]
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: alloc::vec![0.0; capacity].into_boxed_slice(),
            write_idx: 0,
            count: 0,
        }
    }

    pub fn clear(&mut self) {
        self.count = 0;
        self.write_idx = 0;
    }

    pub fn capacity(&self) -> usize {
        self.buffer.len()
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn is_full(&self) -> bool {
        self.len() == self.capacity()
    }

    pub fn push(&mut self, sample: f32) -> Option<f32> {
        let old = if self.count < self.buffer.len() {
            self.count += 1;
            None
        } else {
            Some(self.buffer[self.write_idx])
        };
        self.buffer[self.write_idx] = sample;
        self.write_idx = (self.write_idx + 1) % self.buffer.len();
        old
    }

    pub fn iter(&self) -> impl Iterator<Item = f32> + Clone + '_ {
        let start = self.write_idx;
        let end = start + self.count;
        let len = self.buffer.len();
        let buffer = &self.buffer;
        (start..end).map(move |i| buffer[i % len])
    }
}
