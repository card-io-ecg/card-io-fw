pub mod ram;

fn size_to_bytes(size: usize) -> u32 {
    match size {
        0..=255 => 1,
        256..=65535 => 2,
        65536..=16777215 => 3,
        16777216..=4294967295 => 4,
        _ => unreachable!(),
    }
}

pub(crate) trait StoragePrivate: StorageMedium {
    fn block_size_bytes() -> u32 {
        size_to_bytes(Self::BLOCK_SIZE)
    }

    fn block_count_bytes() -> u32 {
        size_to_bytes(Self::BLOCK_COUNT)
    }

    fn block_header() -> u32 {
        // 2 bytes constant (FS version)
        0xBA01 << 16
        // 1 byte layout info
            | Self::block_size_bytes() << 14 // 2 bits
            | Self::block_count_bytes() << 10 // 4 bits
            | match Self::WRITE_GRANULARITY {
                WriteGranularity::Bit => 0,
                WriteGranularity::Word => 1,
            } << 8 // 1 bit

        // 1 byte reserved
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

pub enum WriteGranularity {
    Bit,
    Word,
}
