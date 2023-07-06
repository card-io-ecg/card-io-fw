//! Compressing i32 buffer
//!
//! This buffer tries to compress a sequence of i32 values by storing the varint-encoded
//! difference from the last. This is useful for storing a sequence of values that are
//! close to each other, such as a sequence of samples from a sensor.

use crate::buffer::Buffer;

pub struct CompressingBuffer<const N: usize> {
    /// Oldest element
    first_element: i32,

    /// Newest element
    last_element: i32,
    element_count: usize,

    buffer: Buffer<u8, N>,
}

impl<const N: usize> CompressingBuffer<N> {
    pub const EMPTY: Self = Self::new();

    pub const fn new() -> Self {
        Self {
            first_element: 0,
            last_element: 0,
            element_count: 0,
            buffer: Buffer::EMPTY,
        }
    }

    fn encode<'a>(&mut self, value: i32, buffer: &'a mut [u8]) -> &'a [u8] {
        const fn zigzag_encode(val: i32) -> u32 {
            ((val << 1) ^ (val >> 31)) as u32
        }
        let mut value = zigzag_encode(value);
        let mut idx = 0;
        while value >= 0x80 {
            buffer[idx] = (value as u8) | 0x80;
            value >>= 7;
            idx += 1;
        }
        buffer[idx] = value as u8;
        &buffer[..=idx]
    }

    pub fn push(&mut self, item: i32) {
        if self.is_empty() {
            self.first_element = item;
            self.last_element = item;
        } else {
            let diff = item - self.first_element;
            let mut buffer = [0; 8];
            let encoded = self.encode(diff, &mut buffer);

            self.first_element = item;

            while self.space() < encoded.len() {
                if self.pop().is_none() {
                    return;
                }
            }

            for byte in encoded {
                self.buffer.push(*byte);
            }
        }
        self.element_count += 1;
    }

    pub fn pop(&mut self) -> Option<i32> {
        const fn zigzag_decode(val: u32) -> i32 {
            (val >> 1) as i32 ^ -((val & 1) as i32)
        }

        if self.is_empty() {
            None
        } else {
            let mut diff = 0;
            let mut idx = 0;
            while let Some(byte) = self.buffer.pop() {
                diff |= ((byte & 0x7F) as u32) << (idx * 7);
                idx += 1;
                if byte & 0x80 == 0 {
                    break;
                }
            }
            let diff = zigzag_decode(diff);
            self.element_count -= 1;

            let last = self.last_element;
            self.last_element += diff;

            Some(last)
        }
    }

    pub fn capacity(&self) -> usize {
        N
    }

    pub fn len(&self) -> usize {
        self.element_count
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn byte_count(&self) -> usize {
        self.buffer.len()
    }

    pub fn space(&self) -> usize {
        N - self.byte_count()
    }

    pub fn clear(&mut self) {
        self.element_count = 0;
        self.first_element = 0;
        self.last_element = 0;
        self.buffer.clear();
    }

    pub fn as_bytes(&self) -> (&[u8], &[u8]) {
        let (a, b) = if self.write_idx > self.bytes {
            let start = self.write_idx - self.bytes;
            let end = self.write_idx;
            (&self.buffer[start..end], &[][..])
        } else {
            let start = N - (self.bytes - self.write_idx);
            (&self.buffer[start..], &self.buffer[..self.write_idx])
        };

        unsafe {
            (
                core::slice::from_raw_parts(
                    a.as_ptr() as *const u8,
                    a.len() * core::mem::size_of::<i32>(),
                ),
                core::slice::from_raw_parts(
                    b.as_ptr() as *const u8,
                    b.len() * core::mem::size_of::<i32>(),
                ),
            )
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn new_buffer_is_empty() {
        let buffer = CompressingBuffer::<100>::new();
        assert!(buffer.is_empty());
        assert_eq!(buffer.len(), 0);
        assert_eq!(buffer.byte_count(), 0);
    }

    #[test]
    fn pushing_an_element_increases_element_count() {
        let mut buffer = CompressingBuffer::<100>::new();
        buffer.push(1);
        assert_eq!(buffer.len(), 1);
    }

    #[test]
    fn first_element_is_not_stored() {
        let mut buffer = CompressingBuffer::<100>::new();
        buffer.push(1);
        assert_eq!(buffer.byte_count(), 0);
    }

    #[test]
    fn popping_from_empty_buffer_returns_none() {
        let mut buffer = CompressingBuffer::<100>::new();
        assert_eq!(buffer.pop(), None);
    }

    #[test]
    fn popping_returns_last_inserted_element() {
        let mut buffer = CompressingBuffer::<100>::new();
        buffer.push(1);

        assert_eq!(buffer.pop(), Some(1));
    }

    #[test]
    fn popping_reduces_len() {
        let mut buffer = CompressingBuffer::<100>::new();
        buffer.push(1);
        buffer.push(2);
        buffer.pop();

        assert_eq!(buffer.len(), 1);
    }

    #[test]
    fn pop_returns_elements_in_order_of_insertion() {
        let mut buffer = CompressingBuffer::<100>::new();
        buffer.push(442);
        buffer.push(-987);
        buffer.push(65254);

        assert_eq!(buffer.pop(), Some(442));
        assert_eq!(buffer.pop(), Some(-987));
        assert_eq!(buffer.pop(), Some(65254));
    }

    #[test]
    fn storing_small_differences_is_more_efficient() {
        let mut buffer = CompressingBuffer::<100>::new();
        buffer.push(0);
        buffer.push(-6);
        buffer.push(32);
        buffer.push(0);
        buffer.push(-6);
        buffer.push(32);
        buffer.push(0);
        buffer.push(-6);
        buffer.push(32);

        // -1 because we explicitly don't store the first element
        assert!(buffer.byte_count() < (buffer.len() - 1) * 4);
    }

    #[test]
    fn overwriting_stays_consistent() {
        let mut buffer = CompressingBuffer::<100>::new();

        for input in 0..500 {
            buffer.push(6 * input);
        }

        buffer.push(32);
        buffer.push(0);
        buffer.push(-6);
        buffer.push(32);

        let mut output = Vec::new();
        while let Some(item) = buffer.pop() {
            output.push(item);
        }

        assert_eq!(&output[output.len() - 4..], [32, 0, -6, 32]);
    }
}
