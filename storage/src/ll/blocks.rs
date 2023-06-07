use crate::medium::{StorageMedium, StoragePrivate};

pub struct BlockHeader {
    header: u32,
    erase_count: u32,
}
impl BlockHeader {
    const HEADER_BYTES: usize = 8;

    async fn read<M: StorageMedium>(medium: &mut M, block: usize) -> Result<Self, ()> {
        let mut header_bytes = [0; 4];
        let mut erase_count_bytes = [0; 4];

        medium.read(block, 0, &mut header_bytes).await?;
        medium.read(block, 4, &mut erase_count_bytes).await?;

        Ok(Self {
            header: u32::from_le_bytes(header_bytes),
            erase_count: u32::from_le_bytes(erase_count_bytes),
        })
    }

    fn new<M: StorageMedium>(new_erase_count: u32) -> Self {
        Self {
            header: M::block_header(),
            erase_count: new_erase_count,
        }
    }

    fn into_bytes(self) -> [u8; Self::HEADER_BYTES] {
        let mut bytes = [0; Self::HEADER_BYTES];

        bytes[0..4].copy_from_slice(&self.header.to_le_bytes());
        bytes[4..8].copy_from_slice(&self.erase_count.to_le_bytes());

        bytes
    }

    async fn write<M: StorageMedium>(self, block: usize, medium: &mut M) -> Result<(), ()> {
        let bytes = self.into_bytes();
        medium.write(block, 0, &bytes).await
    }

    fn is_empty(&self) -> bool {
        self.header == u32::MAX && self.erase_count == u32::MAX
    }

    fn is_expected<M: StorageMedium>(&self) -> bool {
        self.header == M::block_header()
    }
}

pub(crate) struct BlockOps<'a, M> {
    medium: &'a mut M,
}

impl<'a, M: StorageMedium> BlockOps<'a, M> {
    pub fn new(medium: &'a mut M) -> Self {
        Self { medium }
    }

    pub async fn is_block_data_empty(&mut self, block: usize) -> Result<bool, ()> {
        fn is_written(byte: &u8) -> bool {
            *byte != 0xFF
        }

        let mut buffer = [0; 4];

        let block_data_size = M::BLOCK_SIZE - BlockHeader::HEADER_BYTES;

        for offset in (0..block_data_size).step_by(buffer.len()) {
            let remaining_bytes = block_data_size - offset;
            let len = remaining_bytes.min(buffer.len());

            let buffer = &mut buffer[0..len];

            // TODO: impl should cache consecutive small reads
            self.read_data(block, offset, buffer).await?;
            if buffer.iter().any(is_written) {
                return Ok(false);
            }
        }

        Ok(true)
    }

    pub async fn read_header(&mut self, block: usize) -> Result<BlockHeader, ()> {
        BlockHeader::read(self.medium, block).await
    }

    pub async fn format_block(&mut self, block: usize) -> Result<(), ()> {
        let header = self.read_header(block).await?;

        let mut erase = true;
        let mut new_erase_count = 0;

        if header.is_empty() {
            if self.is_block_data_empty(block).await? {
                erase = false;
            }
        } else if header.is_expected::<M>() {
            if self.is_block_data_empty(block).await? {
                // Block is already formatted
                return Ok(());
            }

            new_erase_count = match header.erase_count.checked_add(1) {
                Some(count) => count,
                None => {
                    // We can't erase this block, because it has reached the maximum erase count
                    return Err(());
                }
            }
        }

        if erase {
            self.medium.erase(block).await?;
        }

        BlockHeader::new::<M>(new_erase_count)
            .write(block, self.medium)
            .await
    }

    pub async fn format_storage(&mut self) -> Result<(), ()> {
        for block in 0..M::BLOCK_COUNT {
            self.format_block(block).await?;
        }

        Ok(())
    }

    pub async fn write_data(&mut self, block: usize, offset: usize, data: &[u8]) -> Result<(), ()> {
        self.medium
            .write(block, offset + BlockHeader::HEADER_BYTES, data)
            .await
    }

    pub async fn read_data(
        &mut self,
        block: usize,
        offset: usize,
        data: &mut [u8],
    ) -> Result<(), ()> {
        self.medium
            .read(block, offset + BlockHeader::HEADER_BYTES, data)
            .await
    }
}

#[cfg(test)]
mod tests {
    use crate::medium::ram::RamStorage;

    use super::*;

    #[async_std::test]
    async fn test_formatting_empty_block_sets_erase_count_to_0() {
        let mut medium = RamStorage::<256, 32>::new();
        let mut block_ops = BlockOps::new(&mut medium);

        block_ops.format_block(3).await.unwrap();
        assert_eq!(block_ops.read_header(3).await.unwrap().erase_count, 0);
    }

    #[async_std::test]
    async fn test_format_storage_formats_every_block() {
        let mut medium = RamStorage::<256, 32>::new();
        let mut block_ops = BlockOps::new(&mut medium);

        block_ops.format_storage().await.unwrap();
        for block in 0..RamStorage::<256, 32>::BLOCK_COUNT {
            assert_eq!(block_ops.read_header(block).await.unwrap().erase_count, 0);
        }
    }

    #[async_std::test]
    async fn test_formatting_formatted_but_empty_block_does_not_increase_erase_count() {
        let mut medium = RamStorage::<256, 32>::new();
        let mut block_ops = BlockOps::new(&mut medium);

        block_ops.format_block(3).await.unwrap();
        assert_eq!(block_ops.read_header(3).await.unwrap().erase_count, 0);

        block_ops.format_block(3).await.unwrap();
        assert_eq!(block_ops.read_header(3).await.unwrap().erase_count, 0);
    }

    #[async_std::test]
    async fn test_formatting_written_block_increases_erase_count() {
        let mut medium = RamStorage::<256, 32>::new();
        let mut block_ops = BlockOps::new(&mut medium);

        block_ops.format_block(3).await.unwrap();
        assert_eq!(block_ops.read_header(3).await.unwrap().erase_count, 0);

        block_ops.write_data(3, 5, &[1, 2, 3]).await.unwrap();

        block_ops.format_block(3).await.unwrap();
        assert_eq!(block_ops.read_header(3).await.unwrap().erase_count, 1);
    }

    #[async_std::test]
    async fn test_written_data_can_be_read() {
        let mut medium = RamStorage::<256, 32>::new();
        let mut block_ops = BlockOps::new(&mut medium);

        block_ops.format_block(3).await.unwrap();

        block_ops.write_data(3, 5, &[1, 2, 3]).await.unwrap();

        let mut buffer = [0; 8];
        block_ops.read_data(3, 0, &mut buffer).await.unwrap();
        assert_eq!(buffer, [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 1, 2, 3]);
    }
}
