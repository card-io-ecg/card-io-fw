//! Sliding window

pub struct SlidingWindow<const N: usize> {
    buffer: [f32; N],
    idx: usize,
    full: bool,
}

impl<const N: usize> Default for SlidingWindow<N> {
    #[inline(always)]
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl<const N: usize> SlidingWindow<N> {
    pub const DEFAULT: Self = Self {
        buffer: [0.0; N],
        idx: 0,
        full: false,
    };

    #[inline(always)]
    pub const fn new() -> Self {
        Self::DEFAULT
    }

    pub fn from_initial(buffer: [f32; N]) -> Self {
        Self {
            buffer,
            idx: 0,
            full: true,
        }
    }

    pub fn clear(&mut self) {
        self.idx = 0;
        self.full = false;
    }

    pub fn len(&self) -> usize {
        if self.full {
            N
        } else {
            self.idx
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn is_full(&self) -> bool {
        self.full
    }

    pub fn push(&mut self, sample: f32) -> Option<f32> {
        let old = self.full.then_some(self.buffer[self.idx]);

        self.buffer[self.idx] = sample;
        self.idx = (self.idx + 1) % self.buffer.len();
        if self.idx == 0 {
            self.full = true;
        }

        old
    }

    pub fn iter(&self) -> impl Iterator<Item = f32> + Clone + '_ {
        (self.idx..self.buffer.len())
            .chain(0..self.idx)
            .map(|i| self.buffer[i])
            .take(self.len())
    }
}
