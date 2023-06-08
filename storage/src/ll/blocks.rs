use core::marker::PhantomData;

use crate::medium::{StorageMedium, StoragePrivate, WriteGranularity};

#[derive(Clone, Copy, PartialEq, PartialOrd)]
pub enum BlockType {
    Metadata,
    Data,
}

#[derive(Clone, Copy, PartialEq, PartialOrd)]
enum BlockHeaderKind {
    Empty,
    Unknown,
    Known(BlockType),
}

impl BlockHeaderKind {
    fn from_bytes<M: StorageMedium>(header_bytes: [u8; 4]) -> BlockHeaderKind {
        let options = [
            Self::Known(BlockType::Data),
            Self::Known(BlockType::Metadata),
            Self::Empty,
        ];
        for option in options.iter() {
            if header_bytes == option.to_le_bytes::<M>() {
                return *option;
            }
        }

        Self::Unknown
    }

    fn to_le_bytes<M: StorageMedium>(self) -> [u8; 4] {
        let header = match self {
            BlockHeaderKind::Known(ty) => {
                let fs_info = 0xBA01_0000; // 2 bytes constant

                let layout_info = M::block_size_bytes() << 14 // 2 bits
                | M::block_count_bytes() << 10 // 4 bits
                | match M::WRITE_GRANULARITY {
                    WriteGranularity::Bit => 0,
                    WriteGranularity::Word => 1,
                } << 8; // 1 bit

                let blk_ty = match ty {
                    BlockType::Metadata => 0x55,
                    BlockType::Data => 0xaa,
                };

                fs_info | layout_info | blk_ty
            }
            BlockHeaderKind::Empty | BlockHeaderKind::Unknown => u32::MAX,
        };

        header.to_le_bytes()
    }

    /// Returns `true` if the block header kind is [`Empty`].
    ///
    /// [`Empty`]: BlockHeaderKind::Empty
    #[must_use]
    fn is_empty(self) -> bool {
        matches!(self, Self::Empty)
    }

    /// Returns `true` if the block header kind is [`Known`].
    ///
    /// [`Known`]: BlockHeaderKind::Known
    #[must_use]
    fn is_known(self) -> bool {
        matches!(self, Self::Known(..))
    }

    /// Returns `true` if the block header kind is [`Unknown`].
    ///
    /// [`Unknown`]: BlockHeaderKind::Unknown
    #[must_use]
    fn is_unknown(self) -> bool {
        matches!(self, Self::Unknown)
    }

    fn as_known(self) -> Option<BlockType> {
        if let Self::Known(v) = self {
            Some(v)
        } else {
            None
        }
    }
}

pub struct BlockHeader<M: StorageMedium> {
    header: BlockHeaderKind,
    erase_count: u32,
    _medium: PhantomData<M>,
}
const HEADER_BYTES: usize = 8;

impl<M: StorageMedium> BlockHeader<M> {
    async fn read(medium: &mut M, block: usize) -> Result<Self, ()> {
        let mut header_bytes = [0; 4];
        let mut erase_count_bytes = [0; 4];

        medium.read(block, 0, &mut header_bytes).await?;
        medium.read(block, 4, &mut erase_count_bytes).await?;

        Ok(Self {
            header: BlockHeaderKind::from_bytes::<M>(header_bytes),
            erase_count: u32::from_le_bytes(erase_count_bytes),
            _medium: PhantomData,
        })
    }

    fn new(ty: BlockType, new_erase_count: u32) -> Self {
        Self {
            header: BlockHeaderKind::Known(ty),
            erase_count: new_erase_count,
            _medium: PhantomData,
        }
    }

    fn block_header(ty: BlockType) -> u32 {
        // 2 bytes constant (FS version)
        0xBA01 << 16
        // 1 byte layout info
            | M::block_size_bytes() << 14 // 2 bits
            | M::block_count_bytes() << 10 // 4 bits
            | match M::WRITE_GRANULARITY {
                WriteGranularity::Bit => 0,
                WriteGranularity::Word => 1,
            } << 8 // 1 bit

        // 1 byte block type
        | match ty {
            BlockType::Metadata => 0x55,
            BlockType::Data => 0xaa,
        }
    }

    fn into_bytes(self) -> [u8; HEADER_BYTES] {
        let mut bytes = [0; HEADER_BYTES];

        bytes[0..4].copy_from_slice(&self.header.to_le_bytes::<M>());
        bytes[4..8].copy_from_slice(&self.erase_count.to_le_bytes());

        bytes
    }

    async fn write(self, block: usize, medium: &mut M) -> Result<(), ()> {
        let bytes = self.into_bytes();
        medium.write(block, 0, &bytes).await
    }

    fn is_empty(&self) -> bool {
        self.header.is_empty() && self.erase_count == u32::MAX
    }

    // TODO: read header kind
    fn kind(&self) -> BlockHeaderKind {
        self.header
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

        let block_data_size = M::BLOCK_SIZE - HEADER_BYTES;

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

    pub async fn read_header(&mut self, block: usize) -> Result<BlockHeader<M>, ()> {
        BlockHeader::read(self.medium, block).await
    }

    pub async fn format_block(&mut self, block: usize, ty: BlockType) -> Result<(), ()> {
        let header = self.read_header(block).await?;

        let mut erase = true;
        let mut new_erase_count = 0;

        if header.is_empty() {
            if self.is_block_data_empty(block).await? {
                erase = false;
            }
        } else if let Some(current_ty) = header.kind().as_known() {
            if current_ty == ty {
                if self.is_block_data_empty(block).await? {
                    // Block is already formatted
                    return Ok(());
                }
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

        BlockHeader::new(ty, new_erase_count)
            .write(block, self.medium)
            .await
    }

    pub async fn format_storage(&mut self, metadata_blocks: usize) -> Result<(), ()> {
        for block in 0..metadata_blocks {
            self.format_block(block, BlockType::Metadata).await?;
        }
        for block in metadata_blocks..M::BLOCK_COUNT {
            self.format_block(block, BlockType::Data).await?;
        }

        Ok(())
    }

    pub async fn write_data(&mut self, block: usize, offset: usize, data: &[u8]) -> Result<(), ()> {
        self.medium.write(block, offset + HEADER_BYTES, data).await
    }

    pub async fn read_data(
        &mut self,
        block: usize,
        offset: usize,
        data: &mut [u8],
    ) -> Result<(), ()> {
        self.medium.read(block, offset + HEADER_BYTES, data).await
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

        block_ops.format_block(3, BlockType::Data).await.unwrap();
        assert_eq!(block_ops.read_header(3).await.unwrap().erase_count, 0);
    }

    #[async_std::test]
    async fn test_format_storage_formats_every_block() {
        let mut medium = RamStorage::<256, 32>::new();
        let mut block_ops = BlockOps::new(&mut medium);

        block_ops.format_storage(4).await.unwrap();
        for block in 0..RamStorage::<256, 32>::BLOCK_COUNT {
            assert_eq!(block_ops.read_header(block).await.unwrap().erase_count, 0);
        }
    }

    #[async_std::test]
    async fn test_formatting_formatted_but_empty_block_does_not_increase_erase_count() {
        let mut medium = RamStorage::<256, 32>::new();
        let mut block_ops = BlockOps::new(&mut medium);

        block_ops
            .format_block(3, BlockType::Metadata)
            .await
            .unwrap();
        assert_eq!(block_ops.read_header(3).await.unwrap().erase_count, 0);

        block_ops
            .format_block(3, BlockType::Metadata)
            .await
            .unwrap();
        assert_eq!(block_ops.read_header(3).await.unwrap().erase_count, 0);
    }

    #[async_std::test]
    async fn test_changing_block_type_increases_erase_count() {
        let mut medium = RamStorage::<256, 32>::new();
        let mut block_ops = BlockOps::new(&mut medium);

        block_ops
            .format_block(3, BlockType::Metadata)
            .await
            .unwrap();
        assert_eq!(block_ops.read_header(3).await.unwrap().erase_count, 0);

        block_ops.format_block(3, BlockType::Data).await.unwrap();
        assert_eq!(block_ops.read_header(3).await.unwrap().erase_count, 1);
    }

    #[async_std::test]
    async fn test_formatting_written_block_increases_erase_count() {
        let mut medium = RamStorage::<256, 32>::new();
        let mut block_ops = BlockOps::new(&mut medium);

        block_ops
            .format_block(3, BlockType::Metadata)
            .await
            .unwrap();
        assert_eq!(block_ops.read_header(3).await.unwrap().erase_count, 0);

        block_ops.write_data(3, 5, &[1, 2, 3]).await.unwrap();

        block_ops
            .format_block(3, BlockType::Metadata)
            .await
            .unwrap();
        assert_eq!(block_ops.read_header(3).await.unwrap().erase_count, 1);
    }

    #[async_std::test]
    async fn test_written_data_can_be_read() {
        let mut medium = RamStorage::<256, 32>::new();
        let mut block_ops = BlockOps::new(&mut medium);

        block_ops
            .format_block(3, BlockType::Metadata)
            .await
            .unwrap();

        block_ops.write_data(3, 5, &[1, 2, 3]).await.unwrap();

        let mut buffer = [0; 8];
        block_ops.read_data(3, 0, &mut buffer).await.unwrap();
        assert_eq!(buffer, [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 1, 2, 3]);
    }
}
