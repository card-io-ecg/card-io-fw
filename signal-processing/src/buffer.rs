use core::mem::MaybeUninit;

pub struct Buffer<T: Copy, const N: usize> {
    write_idx: usize,
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
            write_idx: 0,
            full: false,
            buffer: [MaybeUninit::uninit(); N],
        }
    }

    pub fn clear(&mut self) {
        self.write_idx = 0;
        self.full = false;
    }

    pub fn len(&self) -> usize {
        if self.full {
            N
        } else {
            self.write_idx
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
            .then_some(unsafe { self.buffer[self.write_idx].assume_init() });

        self.buffer[self.write_idx] = MaybeUninit::new(sample);
        self.write_idx = (self.write_idx + 1) % self.buffer.len();
        if self.write_idx == 0 {
            self.full = true;
        }

        old
    }

    pub fn iter(&self) -> impl Iterator<Item = T> + Clone + '_ {
        let (start, end) = if !self.full {
            (&self.buffer[0..self.write_idx], &[][..])
        } else {
            (
                &self.buffer[self.write_idx..],
                &self.buffer[0..self.write_idx],
            )
        };

        start
            .iter()
            .chain(end.iter())
            .map(|e| unsafe { e.assume_init() })
    }
}

#[cfg(test)]
mod test {
    #[test]
    fn new_buffer_is_empty() {
        let buffer = super::Buffer::<u8, 4>::new();
        assert!(buffer.is_empty());
    }

    #[test]
    fn push_increases_len() {
        let mut buffer = super::Buffer::<u8, 4>::new();
        let old = buffer.push(1);
        assert_eq!(buffer.len(), 1);
        assert_eq!(old, None);
    }

    #[test]
    fn push_into_full_does_not_increase_length() {
        let mut buffer = super::Buffer::<u8, 4>::new();
        buffer.push(1);
        buffer.push(2);
        buffer.push(3);
        buffer.push(4);
        let old = buffer.push(5);
        assert_eq!(buffer.len(), 4);
        assert_eq!(old, Some(1));
    }

    #[test]
    fn iter_returns_items_in_insertion_order() {
        let mut buffer = super::Buffer::<u8, 4>::new();
        buffer.push(1);
        buffer.push(2);
        buffer.push(3);

        let vector = buffer.iter().collect::<Vec<_>>();
        assert_eq!(vector, vec![1, 2, 3]);
    }

    #[test]
    fn iter_returns_items_in_insertion_order2() {
        let mut buffer = super::Buffer::<u8, 4>::new();
        buffer.push(1);
        buffer.push(2);
        buffer.push(3);
        buffer.push(4);
        buffer.push(5);

        let vector = buffer.iter().collect::<Vec<_>>();
        assert_eq!(vector, vec![2, 3, 4, 5]);
    }
}
