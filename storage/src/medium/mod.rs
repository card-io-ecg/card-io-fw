use crate::StorageError;

pub mod cache;
pub mod ram;
pub mod ram_aligned;
pub mod ram_nor_emulating;

fn size_to_bytes(size: usize) -> usize {
    match size {
        0..=255 => 1,
        256..=65535 => 2,
        65536..=16777215 => 3,
        16777216..=4294967295 => 4,
        _ => unreachable!(),
    }
}

pub(crate) trait StoragePrivate: StorageMedium {
    fn block_size_bytes() -> usize {
        size_to_bytes(Self::BLOCK_SIZE)
    }

    fn block_count_bytes() -> usize {
        size_to_bytes(Self::BLOCK_COUNT)
    }

    fn object_size_bytes() -> usize {
        size_to_bytes(Self::BLOCK_SIZE)
    }

    fn object_location_bytes() -> usize {
        Self::block_count_bytes() + Self::block_size_bytes()
    }

    fn align(size: usize) -> usize {
        match Self::WRITE_GRANULARITY {
            WriteGranularity::Bit => size,
            WriteGranularity::Word(len) => {
                let remainder = size % len;
                if remainder == 0 {
                    size
                } else {
                    size + len - remainder
                }
            }
        }
    }
}

impl<T> StoragePrivate for T where T: StorageMedium {}

pub trait StorageMedium {
    const BLOCK_SIZE: usize;
    const BLOCK_COUNT: usize;

    /// The smallest writeable unit. Determines how object flags are stored.
    const WRITE_GRANULARITY: WriteGranularity;

    async fn erase(&mut self, block: usize) -> Result<(), StorageError>;
    async fn read(
        &mut self,
        block: usize,
        offset: usize,
        data: &mut [u8],
    ) -> Result<(), StorageError>;
    async fn write(&mut self, block: usize, offset: usize, data: &[u8])
        -> Result<(), StorageError>;
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum WriteGranularity {
    Bit,
    Word(usize),
}

impl WriteGranularity {
    pub const fn width(self) -> usize {
        match self {
            WriteGranularity::Bit => 1,
            WriteGranularity::Word(len) => len,
        }
    }
}
