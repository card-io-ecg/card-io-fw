pub mod cache;
pub mod ram;

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

    fn object_status_bytes() -> usize {
        match Self::WRITE_GRANULARITY {
            WriteGranularity::Bit => 1,
            WriteGranularity::Word(len) => 3 * len,
        }
    }

    fn object_size_bytes() -> usize {
        size_to_bytes(Self::BLOCK_SIZE)
    }

    fn object_header_bytes() -> usize {
        Self::align(Self::object_status_bytes()) // status
            + Self::align(Self::object_size_bytes()) // payload size
            + Self::align(Self::object_location_bytes()) // reserved
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

    async fn erase(&mut self, block: usize) -> Result<(), ()>;
    async fn read(&mut self, block: usize, offset: usize, data: &mut [u8]) -> Result<(), ()>;
    async fn write(&mut self, block: usize, offset: usize, data: &[u8]) -> Result<(), ()>;
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
