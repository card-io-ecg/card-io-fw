use core::{mem::MaybeUninit, ops::Range};

pub struct Buffer<T: Copy, const N: usize> {
    write_idx: usize,
    count: usize,
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
            count: 0,
            buffer: [MaybeUninit::uninit(); N],
        }
    }

    pub fn clear(&mut self) {
        self.write_idx = 0;
        self.count = 0;
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub const fn capacity(&self) -> usize {
        N
    }

    pub fn is_full(&self) -> bool {
        self.count == N
    }

    pub fn push(&mut self, sample: T) -> Option<T> {
        let old = self
            .is_full()
            .then_some(unsafe { self.buffer[self.write_idx].assume_init() });

        self.buffer[self.write_idx] = MaybeUninit::new(sample);
        self.write_idx = (self.write_idx + 1) % self.buffer.len();
        if !self.is_full() {
            self.count += 1;
        }

        old
    }

    fn read_index(&self) -> usize {
        (self.write_idx + N - self.count) % N
    }

    pub fn pop(&mut self) -> Option<T> {
        if self.is_empty() {
            None
        } else {
            let old_byte = self.buffer[self.read_index()];
            self.count -= 1;
            Some(unsafe { old_byte.assume_init() })
        }
    }

    fn slice_idxs(&self) -> (Range<usize>, Range<usize>) {
        let read_index = self.read_index();

        if read_index < self.write_idx {
            (read_index..self.write_idx, 0..0)
        } else if !self.is_empty() {
            (read_index..N, 0..self.write_idx)
        } else {
            (0..0, 0..0)
        }
    }

    pub fn as_slices(&self) -> (&[T], &[T]) {
        let (start_range, end_range) = self.slice_idxs();

        let start = &self.buffer[start_range];
        let end = &self.buffer[end_range];

        unsafe {
            (
                core::slice::from_raw_parts(start.as_ptr() as *const T, start.len()),
                core::slice::from_raw_parts(end.as_ptr() as *const T, end.len()),
            )
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = T> + Clone + '_ {
        let (start, end) = self.as_slices();

        start.iter().chain(end.iter()).copied()
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
    fn pop_returns_none_if_empty() {
        let mut buffer = super::Buffer::<u8, 4>::new();
        assert_eq!(buffer.pop(), None);
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

    #[test]
    fn pop_removes_and_returns_oldest() {
        let mut buffer = super::Buffer::<u8, 4>::new();
        buffer.push(1);
        buffer.push(2);
        buffer.push(3);
        buffer.push(4);
        buffer.push(5);

        assert_eq!(buffer.pop(), Some(2));
        assert_eq!(buffer.pop(), Some(3));

        assert_eq!(buffer.len(), 2);

        assert_eq!(buffer.pop(), Some(4));
        assert_eq!(buffer.pop(), Some(5));

        assert!(buffer.is_empty());
    }

    #[test]
    fn iter_does_not_return_popped() {
        let mut buffer = super::Buffer::<u8, 4>::new();
        buffer.push(1);
        buffer.push(2);
        buffer.push(3);
        buffer.push(4);
        buffer.push(5);

        assert_eq!(buffer.pop(), Some(2));
        assert_eq!(buffer.pop(), Some(3));

        let vector = buffer.iter().collect::<Vec<_>>();
        assert_eq!(vector, vec![4, 5]);
    }
}
