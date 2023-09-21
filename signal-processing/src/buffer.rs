use core::{convert::Infallible, mem::MaybeUninit, ops::Range};

use embedded_io::{blocking::Read, Io};

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

    pub fn is_contiguous(&self) -> bool {
        let (_head, tail) = self.slice_idxs();
        tail.is_empty()
    }

    pub fn make_contiguous(&mut self) -> &[T]
    where
        T: core::fmt::Debug,
    {
        if !self.is_contiguous() {
            let (head, tail) = self.slice_idxs();

            let head_start = head.start;
            let head_end = head.end;

            let tail_start = tail.start;
            let tail_end = tail.end;

            let head_count = head_end - head_start;
            let tail_count = tail_end - tail_start;
            let buffer_count = self.len();
            let free_count = self.capacity() - buffer_count;

            // This algorithm is based on std::collections::VecDeque::make_contiguous
            if free_count >= head_count {
                // [B C . . A]
                //  ^       ^ head
                //  | tail
                // Result: [A B C . .]

                self.buffer.copy_within(tail_start..tail_end, head_count);
                self.buffer.copy_within(head_start..head_end, 0);

                self.write_idx = buffer_count;
            } else if free_count >= tail_count {
                // [D . A B C]
                //  ^   ^ head
                //  | tail
                // Result: [. A B C D]

                self.buffer.copy_within(head_start..head_end, tail_count);
                self.buffer
                    .copy_within(tail, head_end - head_start + tail_count);

                self.write_idx = 0;
            } else {
                // move slices next to each other, then rotate

                if head_count >= tail_count {
                    // [D E . A B C]
                    //  ^     ^ head
                    //  | tail
                    // After move:   [. D E A B C] tail < head -> rotate left
                    // After rotate: [. A B C D E]
                    self.buffer.copy_within(tail_start..tail_end, free_count);
                    let buffer = &mut self.buffer[free_count..][..buffer_count];
                    buffer.rotate_left(tail_count);

                    self.write_idx = 0;
                } else {
                    // [C D E . A B]
                    //  ^       ^ head
                    //  | tail
                    // After move:   [C D E A B .] tail < head -> rotate left
                    // After rotate: [A B C D E .]
                    self.buffer.copy_within(head_start..head_end, tail_count);
                    let buffer = &mut self.buffer[..buffer_count];
                    buffer.rotate_right(head_count);

                    self.write_idx = buffer_count;
                }
            }
        }

        self.as_slices().0
    }
}

impl<T: Copy, const N: usize> Io for Buffer<T, N> {
    type Error = Infallible;
}

impl<const N: usize> Read for Buffer<u8, N> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let mut written = 0;
        while written < buf.len() {
            if let Some(sample) = self.pop() {
                buf[written] = sample;
                written += 1;
            } else {
                break;
            }
        }

        Ok(written)
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

    #[test]
    fn make_contiguous_rearranges_internal_buffer_1() {
        let mut buffer = super::Buffer::<u8, 5>::new();
        buffer.push(1);
        buffer.push(2);
        buffer.push(3);
        buffer.push(4);
        buffer.push(5);
        buffer.push(6);
        buffer.push(7);

        buffer.pop();
        buffer.pop();

        // Buffer layout:
        // [6 7 . . 5]
        //  ^       ^ head
        //  | tail

        assert!(!buffer.is_contiguous());

        let contiguous = buffer.make_contiguous();
        assert_eq!([5, 6, 7], contiguous);

        assert!(buffer.is_contiguous());
    }

    #[test]
    fn make_contiguous_rearranges_internal_buffer_2() {
        let mut buffer = super::Buffer::<u8, 5>::new();
        buffer.push(1);
        buffer.push(2);
        buffer.push(3);
        buffer.push(4);
        buffer.push(5);
        buffer.push(6);

        buffer.pop();

        // Buffer layout:
        // [6 . 3 4 5]
        //  ^   ^ head
        //  | tail

        assert!(!buffer.is_contiguous());

        let contiguous = buffer.make_contiguous();
        assert_eq!([3, 4, 5, 6], contiguous);

        assert!(buffer.is_contiguous());
    }

    #[test]
    fn make_contiguous_rearranges_internal_buffer_3() {
        let mut buffer = super::Buffer::<u8, 6>::new();
        buffer.push(1);
        buffer.push(2);
        buffer.push(3);
        buffer.push(4);
        buffer.push(5);
        buffer.push(6);
        buffer.push(7);
        buffer.push(8);

        buffer.pop();

        // Buffer layout:
        // [7 8 . 4 5 6]
        //  ^     ^ head
        //  | tail

        assert!(!buffer.is_contiguous());

        let contiguous = buffer.make_contiguous();
        assert_eq!([4, 5, 6, 7, 8], contiguous);

        assert!(buffer.is_contiguous());
    }

    #[test]
    fn make_contiguous_rearranges_internal_buffer_4() {
        let mut buffer = super::Buffer::<u8, 6>::new();
        buffer.push(1);
        buffer.push(2);
        buffer.push(3);
        buffer.push(4);
        buffer.push(5);
        buffer.push(6);
        buffer.push(7);
        buffer.push(8);
        buffer.push(9);

        buffer.pop();

        // Buffer layout:
        // [7 8 9 . 5 6]
        //  ^     ^ head
        //  | tail

        assert!(!buffer.is_contiguous());

        let contiguous = buffer.make_contiguous();
        assert_eq!([5, 6, 7, 8, 9], contiguous);

        assert!(buffer.is_contiguous());
    }
}
