use crate::{
    medium::{StorageMedium, WriteGranularity},
    StorageError,
};

pub struct Counters<P>
where
    P: StorageMedium,
{
    medium: P,
    pub erase_count: usize,
    pub read_count: usize,
    pub write_count: usize,
}

impl<P> StorageMedium for Counters<P>
where
    P: StorageMedium,
{
    const BLOCK_SIZE: usize = P::BLOCK_SIZE;
    const BLOCK_COUNT: usize = P::BLOCK_COUNT;
    const WRITE_GRANULARITY: WriteGranularity = P::WRITE_GRANULARITY;

    async fn erase(&mut self, block: usize) -> Result<(), StorageError> {
        self.erase_count = self.erase_count.saturating_add(1);
        self.medium.erase(block).await
    }

    async fn read(
        &mut self,
        block: usize,
        offset: usize,
        data: &mut [u8],
    ) -> Result<(), StorageError> {
        self.read_count = self.read_count.saturating_add(1);
        self.medium.read(block, offset, data).await
    }

    async fn write(
        &mut self,
        block: usize,
        offset: usize,
        data: &[u8],
    ) -> Result<(), StorageError> {
        self.write_count = self.write_count.saturating_add(1);
        self.medium.write(block, offset, data).await
    }
}
