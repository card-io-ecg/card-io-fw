use core::{
    mem::{self, MaybeUninit},
    slice,
};

pub trait ByteReadable {
    fn element_size() -> usize
    where
        Self: Sized,
    {
        mem::size_of::<Self>()
    }
}

pub struct Buffer<T: Copy, const N: usize> {
    idx: usize,
    full: bool,
    buffer: [MaybeUninit<T>; N],
}

impl<T: Copy, const N: usize> Default for Buffer<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Copy, const N: usize> Buffer<T, N> {
    pub const EMPTY: Self = Self::new();

    pub const fn new() -> Self {
        Self {
            idx: 0,
            full: false,
            buffer: [MaybeUninit::uninit(); N],
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

    pub const fn capacity(&self) -> usize {
        N
    }

    pub fn is_full(&self) -> bool {
        self.full
    }

    pub fn push(&mut self, sample: T) -> Option<T> {
        let old = self
            .full
            .then_some(unsafe { self.buffer[self.idx].assume_init() });

        self.buffer[self.idx] = MaybeUninit::new(sample);
        self.idx = (self.idx + 1) % self.buffer.len();
        if self.idx == 0 {
            self.full = true;
        }

        old
    }

    pub fn iter(&self) -> impl Iterator<Item = T> + Clone + '_ {
        (self.idx..self.buffer.len())
            .chain(0..self.idx)
            .map(|i| self.buffer[i])
            .take(self.len())
            .map(|e| unsafe { e.assume_init() })
    }

    pub fn as_bytes(&self) -> (&[u8], &[u8])
    where
        T: ByteReadable + Sized,
    {
        let (mut a, mut b) = self.buffer[..self.len()].split_at(self.idx);
        if a.is_empty() {
            mem::swap(&mut a, &mut b);
        }
        unsafe {
            (
                slice::from_raw_parts(a.as_ptr() as *const u8, a.len() * T::element_size()),
                slice::from_raw_parts(b.as_ptr() as *const u8, b.len() * T::element_size()),
            )
        }
    }
}
