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
