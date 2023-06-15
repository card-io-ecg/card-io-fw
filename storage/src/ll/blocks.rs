use core::marker::PhantomData;

use crate::{
    ll::objects::{ObjectIterator, ObjectState},
    medium::{StorageMedium, StoragePrivate, WriteGranularity},
    StorageError,
};

#[derive(Clone, Copy, PartialEq, PartialOrd, Debug)]
pub enum BlockType {
    Metadata = 0x55,
    Data = 0xAA,
    /// Freshly formatted, untouched block. Can become either Metadata or Data.
    Undefined = 0xFF,
}

#[derive(Clone, Copy, PartialEq, PartialOrd, Debug)]
pub enum BlockHeaderKind {
    Empty,
    Unknown,
    Known(BlockType),
}

impl BlockHeaderKind {
    fn from_bytes<M: StorageMedium>(mut header_bytes: [u8; 12]) -> BlockHeaderKind {
        let options = [
            Self::Known(BlockType::Data),
            Self::Known(BlockType::Metadata),
            Self::Known(BlockType::Undefined),
            Self::Empty,
        ];

        // mask erase count
        header_bytes[4..8].fill(0xFF);

        for option in options.iter() {
            let (expectation, count) = option.to_bytes::<M>();
            if header_bytes[..count] == expectation[..count] {
                return *option;
            }
        }

        Self::Unknown
    }

    fn to_bytes<M: StorageMedium>(self) -> ([u8; 12], usize) {
        let mut bytes = [0xFF; 12];

        if let BlockHeaderKind::Known(ty) = self {
            let layout_info = (M::block_size_bytes() as u8) << 6 // 2 bits
            | (M::block_count_bytes() as u8) << 2 // 4 bits
            | match M::WRITE_GRANULARITY {
                WriteGranularity::Bit => 0,
                WriteGranularity::Word(_) => 1,
            }; // 1 bit

            bytes[0] = 0xBA;
            bytes[1] = 0x01;
            bytes[2] = layout_info;

            match M::WRITE_GRANULARITY {
                WriteGranularity::Bit | WriteGranularity::Word(1) => bytes[3] = ty as u8,
                WriteGranularity::Word(4) => bytes[8] = ty as u8,
                _ => unimplemented!(),
            }
        }

        (bytes, BlockHeader::<M>::byte_count())
    }

    /// Returns `true` if the block header kind is [`Empty`].
    ///
    /// [`Empty`]: BlockHeaderKind::Empty
    #[must_use]
    pub fn is_empty(self) -> bool {
        matches!(self, Self::Empty)
    }

    /// Returns `true` if the block header kind is [`Known`].
    ///
    /// [`Known`]: BlockHeaderKind::Known
    #[must_use]
    pub fn is_known(self) -> bool {
        matches!(self, Self::Known(..))
    }

    /// Returns `true` if the block header kind is [`Unknown`].
    ///
    /// [`Unknown`]: BlockHeaderKind::Unknown
    #[must_use]
    pub fn is_unknown(self) -> bool {
        matches!(self, Self::Unknown)
    }

    pub fn as_known(self) -> Option<BlockType> {
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

impl<M: StorageMedium> core::fmt::Debug for BlockHeader<M> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BlockHeader")
            .field("header", &self.header)
            .field("erase_count", &self.erase_count)
            .finish()
    }
}

impl<M: StorageMedium> Clone for BlockHeader<M> {
    fn clone(&self) -> Self {
        Self {
            header: self.header,
            erase_count: self.erase_count,
            _medium: self._medium,
        }
    }
}

impl<M: StorageMedium> Copy for BlockHeader<M> {}

impl<M: StorageMedium> BlockHeader<M> {
    pub async fn read(medium: &mut M, block: usize) -> Result<Self, StorageError> {
        let mut header_bytes = [0; 12];
        medium.read(block, 0, &mut header_bytes).await?;

        Ok(Self {
            header: BlockHeaderKind::from_bytes::<M>(header_bytes),
            erase_count: u32::from_le_bytes(header_bytes[4..8].try_into().unwrap()),
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

    const fn new_unknown() -> Self {
        Self {
            header: BlockHeaderKind::Unknown,
            erase_count: 0,
            _medium: PhantomData,
        }
    }

    /// Returns the number of bytes in a block header, including the erase count.
    pub const fn byte_count() -> usize {
        match M::WRITE_GRANULARITY {
            WriteGranularity::Bit | WriteGranularity::Word(1) => 8,
            WriteGranularity::Word(4) => 12,
            _ => unimplemented!(),
        }
    }

    fn into_bytes(self) -> ([u8; 12], usize) {
        let (mut bytes, byte_count) = self.header.to_bytes::<M>();

        bytes[4..8].copy_from_slice(&self.erase_count.to_le_bytes());

        (bytes, byte_count)
    }

    async fn write(self, block: usize, medium: &mut M) -> Result<(), StorageError> {
        log::trace!("BlockHeader::write({self:?}, {block})");
        let (bytes, byte_count) = self.into_bytes();
        medium.write(block, 0, &bytes[0..byte_count]).await
    }

    pub fn is_empty(&self) -> bool {
        self.header.is_empty() && self.erase_count == u32::MAX
    }

    pub fn kind(&self) -> BlockHeaderKind {
        self.header
    }

    pub fn set_block_type(&mut self, ty: BlockType) {
        self.header = BlockHeaderKind::Known(ty);
    }
}

/// Block info read when the FS is mounted.
pub struct BlockInfo<M: StorageMedium> {
    pub header: BlockHeader<M>,
    /// Includes the header bytes
    used_bytes: usize,
    /// Indicates whether the block is in a good state and new objects can be allocated in it.
    pub allow_alloc: bool,
}

impl<M: StorageMedium> core::fmt::Debug for BlockInfo<M> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BlockInfo")
            .field("header", &self.header)
            .field("used_bytes", &self.used_bytes)
            .field("allow_alloc", &self.allow_alloc)
            .finish()
    }
}

impl<M: StorageMedium> Clone for BlockInfo<M> {
    fn clone(&self) -> Self {
        Self {
            header: self.header,
            used_bytes: self.used_bytes,
            allow_alloc: self.allow_alloc,
        }
    }
}

impl<M: StorageMedium> Copy for BlockInfo<M> {}

impl<M: StorageMedium> BlockInfo<M> {
    pub const fn new_unknown() -> Self {
        Self {
            header: BlockHeader::new_unknown(),
            used_bytes: 0,
            allow_alloc: false,
        }
    }

    pub fn update_stats_after_erase(&mut self) {
        self.header.erase_count += 1;
        self.used_bytes = BlockHeader::<M>::byte_count();
        self.allow_alloc = true;
    }

    pub fn is_metadata(&self) -> bool {
        self.header.kind() == BlockHeaderKind::Known(BlockType::Metadata)
    }

    pub fn is_empty(&self) -> bool {
        self.used_bytes <= BlockHeader::<M>::byte_count()
    }

    pub fn free_space(&self) -> usize {
        M::BLOCK_SIZE - self.used_bytes
    }

    pub fn add_used_bytes(&mut self, object_total_size: usize) {
        self.used_bytes += M::align(object_total_size);
    }

    pub fn used_bytes(&self) -> usize {
        self.used_bytes
    }
}

pub(crate) struct BlockOps<'a, M> {
    medium: &'a mut M,
}

impl<'a, M: StorageMedium> BlockOps<'a, M> {
    pub fn new(medium: &'a mut M) -> Self {
        Self { medium }
    }

    pub async fn is_block_data_empty(&mut self, block: usize) -> Result<bool, StorageError> {
        fn is_written(byte: &u8) -> bool {
            *byte != 0xFF
        }

        let mut buffer = [0; 4];

        let block_data_size = M::BLOCK_SIZE - BlockHeader::<M>::byte_count();

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

    pub async fn read_header(&mut self, block: usize) -> Result<BlockHeader<M>, StorageError> {
        BlockHeader::read(self.medium, block).await
    }

    pub async fn format_block(&mut self, block: usize) -> Result<(), StorageError> {
        let header = self.read_header(block).await?;

        let mut erase = true;
        let mut new_erase_count = 0;

        if header.is_empty() {
            if self.is_block_data_empty(block).await? {
                erase = false;
            }
        } else if let Some(current_ty) = header.kind().as_known() {
            // Technically one of these checks is enough - if the block is empty, it's Undefined
            if current_ty == BlockType::Undefined && self.is_block_data_empty(block).await? {
                // Block is already formatted
                return Ok(());
            }

            new_erase_count = match header.erase_count.checked_add(1) {
                Some(count) => count,
                None => {
                    // We can't erase this block, because it has reached the maximum erase count
                    return Err(StorageError::FsCorrupted);
                }
            }
        }

        if erase {
            log::trace!("Erasing block {block}");
            self.medium.erase(block).await?;
        }

        BlockHeader::new(BlockType::Undefined, new_erase_count)
            .write(block, self.medium)
            .await
    }

    pub async fn format_storage(&mut self) -> Result<(), StorageError> {
        for block in 0..M::BLOCK_COUNT {
            self.format_block(block).await?;
        }

        Ok(())
    }

    #[cfg(test)]
    pub async fn write_data(
        &mut self,
        block: usize,
        offset: usize,
        data: &[u8],
    ) -> Result<(), StorageError> {
        self.medium
            .write(block, offset + BlockHeader::<M>::byte_count(), data)
            .await
    }

    pub async fn read_data(
        &mut self,
        block: usize,
        offset: usize,
        data: &mut [u8],
    ) -> Result<(), StorageError> {
        self.medium
            .read(block, offset + BlockHeader::<M>::byte_count(), data)
            .await
    }

    pub async fn scan_block(&mut self, block: usize) -> Result<BlockInfo<M>, StorageError> {
        let header = BlockHeader::read(self.medium, block).await?;
        let mut used_bytes = 0;

        let last_object_reliable;

        if header.kind().is_known() {
            let mut iter = ObjectIterator::new::<M>(block);

            let mut last_object_kind = ObjectState::Free;
            while let Some(object) = iter.next(self.medium).await? {
                last_object_kind = object.state();
            }

            used_bytes = iter.current_offset();

            // We disallow allocation until the block is fixed.
            last_object_reliable = last_object_kind != ObjectState::Allocated;

            // TODO: detect if a byte has been written after the last object
        } else {
            if header.kind().is_empty() {
                return Err(StorageError::NotFormatted);
            }
            last_object_reliable = false;
            for offset in 0..M::BLOCK_SIZE {
                let data = &mut [0];
                self.medium.read(block, offset, data).await?;

                if data[0] != 0xFF {
                    used_bytes = offset + 1;
                }
            }
        }

        let info = BlockInfo {
            header,
            used_bytes,
            allow_alloc: last_object_reliable,
        };
        log::trace!("BlockOps::scan_block({block}) -> {info:?}");

        Ok(info)
    }

    pub(crate) async fn set_block_type(
        &mut self,
        block: usize,
        ty: BlockType,
    ) -> Result<(), StorageError> {
        let offset = match M::WRITE_GRANULARITY {
            WriteGranularity::Bit | WriteGranularity::Word(1) => 3,
            WriteGranularity::Word(4) => 8,
            _ => unimplemented!(),
        };

        self.medium.write(block, offset, &[ty as u8]).await
    }
}

#[cfg(test)]
mod tests {
    use crate::{medium::ram_nor_emulating::NorRamStorage, test::init_test};

    use super::*;

    #[async_std::test]
    async fn empty_block_reports_not_formatted() {
        init_test();

        let mut medium = NorRamStorage::<256, 32>::new();
        let mut block_ops = BlockOps::new(&mut medium);

        let result = block_ops
            .scan_block(0)
            .await
            .expect_err("Scan should return error when the block is not formatted");
        assert_eq!(result, StorageError::NotFormatted);
    }

    #[async_std::test]
    async fn test_formatting_empty_block_sets_erase_count_to_0() {
        init_test();

        let mut medium = NorRamStorage::<256, 32>::new();
        let mut block_ops = BlockOps::new(&mut medium);

        block_ops.format_block(3).await.unwrap();
        assert_eq!(block_ops.read_header(3).await.unwrap().erase_count, 0);
    }

    #[async_std::test]
    async fn formatted_block_reports_some_used_bytes() {
        init_test();

        let mut medium = NorRamStorage::<256, 32>::new();
        let mut block_ops = BlockOps::new(&mut medium);

        block_ops.format_block(0).await.unwrap();
        assert_eq!(block_ops.read_header(0).await.unwrap().erase_count, 0);

        let info = block_ops.scan_block(0).await.unwrap();
        assert_eq!(info.used_bytes, 8);
    }

    #[async_std::test]
    async fn test_format_storage_formats_every_block() {
        init_test();

        let mut medium = NorRamStorage::<256, 32>::new();
        let mut block_ops = BlockOps::new(&mut medium);

        block_ops.format_storage().await.unwrap();
        for block in 0..NorRamStorage::<256, 32>::BLOCK_COUNT {
            assert_eq!(block_ops.read_header(block).await.unwrap().erase_count, 0);
        }
    }

    #[async_std::test]
    async fn test_formatting_formatted_but_empty_block_does_not_increase_erase_count() {
        init_test();

        let mut medium = NorRamStorage::<256, 32>::new();
        let mut block_ops = BlockOps::new(&mut medium);

        block_ops.format_block(3).await.unwrap();
        assert_eq!(block_ops.read_header(3).await.unwrap().erase_count, 0);

        block_ops.format_block(3).await.unwrap();
        assert_eq!(block_ops.read_header(3).await.unwrap().erase_count, 0);
    }

    #[async_std::test]
    async fn test_formatting_written_block_increases_erase_count() {
        init_test();

        let mut medium = NorRamStorage::<256, 32>::new();
        let mut block_ops = BlockOps::new(&mut medium);

        block_ops.format_block(3).await.unwrap();
        assert_eq!(block_ops.read_header(3).await.unwrap().erase_count, 0);

        block_ops.write_data(3, 5, &[1, 2, 3]).await.unwrap();

        block_ops.format_block(3).await.unwrap();
        assert_eq!(block_ops.read_header(3).await.unwrap().erase_count, 1);
    }

    #[async_std::test]
    async fn test_written_data_can_be_read() {
        init_test();

        let mut medium = NorRamStorage::<256, 32>::new();
        let mut block_ops = BlockOps::new(&mut medium);

        block_ops.format_block(3).await.unwrap();

        block_ops.write_data(3, 5, &[1, 2, 3]).await.unwrap();

        let mut buffer = [0; 8];
        block_ops.read_data(3, 0, &mut buffer).await.unwrap();
        assert_eq!(buffer, [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 1, 2, 3]);
    }
}
