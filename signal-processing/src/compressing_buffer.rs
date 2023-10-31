//! Compressing i32 buffer
//!
//! This buffer tries to compress a sequence of i32 values by storing the varint-encoded
//! difference from the last. This is useful for storing a sequence of values that are
//! close to each other, such as a sequence of samples from a sensor.

use core::{fmt::Debug, slice};

use embedded_io::{Read, ReadExactError, Write};

use crate::buffer::Buffer;

#[derive(Clone, Copy, Default)]
pub struct EkgFormat {
    previous: i32,
}

impl EkgFormat {
    pub const VERSION: u8 = 0;

    pub const fn new() -> Self {
        Self { previous: 0 }
    }

    pub fn write<W: Write>(&mut self, sample: i32, writer: &mut W) -> Result<usize, W::Error> {
        let diff = sample - self.previous;
        self.previous = sample;

        const fn zigzag_encode(val: i32) -> u32 {
            ((val << 1) ^ (val >> 31)) as u32
        }

        let mut diff = zigzag_encode(diff);

        let mut buffer = [0; 8];
        let mut idx = 0;
        while diff >= 0x80 {
            buffer[idx] = (diff as u8) | 0x80;
            diff >>= 7;
            idx += 1;
        }
        buffer[idx] = diff as u8;
        idx += 1;

        writer.write_all(&buffer[..idx])?;

        Ok(idx)
    }

    pub fn read<R: Read>(&mut self, reader: &mut R) -> Result<Option<i32>, R::Error> {
        const fn zigzag_decode(val: u32) -> i32 {
            (val >> 1) as i32 ^ -((val & 1) as i32)
        }

        let mut diff = 0;
        let mut idx = 0;
        let mut byte = 0;
        loop {
            if let Err(e) = reader.read_exact(slice::from_mut(&mut byte)) {
                match e {
                    ReadExactError::UnexpectedEof => return Ok(None),
                    ReadExactError::Other(e) => return Err(e),
                }
            }
            diff |= ((byte & 0x7F) as u32) << (idx * 7);
            idx += 1;
            if byte & 0x80 == 0 {
                break;
            }
        }
        let diff = zigzag_decode(diff);

        let value = self.previous + diff;
        self.previous = value;

        Ok(Some(value))
    }
}

pub struct CompressingBuffer<const N: usize> {
    reader: EkgFormat,
    writer: EkgFormat,
    element_count: usize,

    buffer: Buffer<u8, N, true>,
}

impl<const N: usize> Debug for CompressingBuffer<N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("CompressingBuffer")
            .field(&self.element_count)
            .finish()
    }
}

#[cfg(feature = "defmt")]
impl<const N: usize> defmt::Format for CompressingBuffer<N> {
    fn format(&self, fmt: defmt::Formatter<'_>) {
        defmt::write!(fmt, "CompressingBuffer({})", self.element_count)
    }
}

#[cfg(all(feature = "defmt", feature = "alloc"))]
impl<const N: usize> defmt::Format for alloc::boxed::Box<CompressingBuffer<N>> {
    fn format(&self, fmt: defmt::Formatter<'_>) {
        self.as_ref().format(fmt)
    }
}

impl<const N: usize> CompressingBuffer<N> {
    pub const EMPTY: Self = Self::new();

    pub const fn new() -> Self {
        Self {
            reader: EkgFormat::new(),
            writer: EkgFormat::new(),
            element_count: 0,
            buffer: Buffer::EMPTY,
        }
    }

    pub fn push(&mut self, item: i32) {
        let mut buffer = [0u8; 8];
        let bytes = unwrap!(self.writer.write(item, &mut &mut buffer[..]));

        while self.space() < bytes {
            if self.pop().is_none() {
                return;
            }
        }

        for byte in buffer.iter().take(bytes).copied() {
            self.buffer.push(byte);
        }

        self.element_count += 1;
    }

    pub fn pop(&mut self) -> Option<i32> {
        let sample = unwrap!(self.reader.read(&mut self.buffer));

        if sample.is_some() {
            self.element_count -= 1;
        }

        sample
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
        self.reader.previous = 0;
        self.writer.previous = 0;
        self.buffer.clear();
    }

    pub fn as_slices(&self) -> (&[u8], &[u8]) {
        self.buffer.as_slices()
    }

    pub fn make_contiguous(&mut self) -> &[u8] {
        self.buffer.make_contiguous()
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
    fn first_element_is_stored() {
        let mut buffer = CompressingBuffer::<100>::new();
        buffer.push(1);
        assert_eq!(buffer.byte_count(), 1);
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

        assert_eq!(buffer.len(), 2);

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
