use crate::medium::{StorageMedium, StoragePrivate, WriteGranularity};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ObjectState {
    Free,      // Implicit
    Allocated, // TODO: make this implicit
    Finalized,
    Deleted,
}

impl ObjectState {
    const FREE_WORDS: &[u8] = &[0xFF; 12];
    const ALLOCATED_WORDS: &[u8] = &[
        0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    ];
    const FINALIZED_WORDS: &[u8] = &[
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF,
    ];
    const DELETED_WORDS: &[u8] = &[0; 12];

    fn is_free(self) -> bool {
        matches!(self, ObjectState::Free)
    }

    fn is_allocated(self) -> bool {
        matches!(self, ObjectState::Allocated)
    }

    fn is_valid(self) -> bool {
        matches!(self, ObjectState::Finalized)
    }

    fn is_deleted(self) -> bool {
        matches!(self, ObjectState::Deleted)
    }

    fn is_used(self) -> bool {
        matches!(
            self,
            ObjectState::Allocated | ObjectState::Finalized | ObjectState::Deleted
        )
    }

    fn into_bits(self) -> u8 {
        match self {
            ObjectState::Free => 0xFF,
            ObjectState::Allocated => 0xFE,
            ObjectState::Finalized => 0xFC,
            ObjectState::Deleted => 0x00,
        }
    }

    fn from_bits(bits: u8) -> Result<Self, ()> {
        match bits {
            0xFF => Ok(ObjectState::Free),
            0xFE => Ok(ObjectState::Allocated),
            0xFC => Ok(ObjectState::Finalized),
            0x00 => Ok(ObjectState::Deleted),
            _ => Err(()),
        }
    }

    fn into_words(self) -> &'static [u8] {
        match self {
            Self::Free => Self::FREE_WORDS,
            Self::Allocated => Self::ALLOCATED_WORDS,
            Self::Finalized => Self::FINALIZED_WORDS,
            Self::Deleted => Self::DELETED_WORDS,
        }
    }

    fn from_words(words: &[u8]) -> Result<Self, ()> {
        match words {
            Self::FREE_WORDS => Ok(Self::Free),
            Self::ALLOCATED_WORDS => Ok(Self::Allocated),
            Self::FINALIZED_WORDS => Ok(Self::Finalized),
            Self::DELETED_WORDS => Ok(Self::Deleted),
            _ => Err(()),
        }
    }

    async fn write<M: StorageMedium>(
        self,
        location: ObjectLocation,
        medium: &mut M,
    ) -> Result<(), ()> {
        match M::WRITE_GRANULARITY {
            WriteGranularity::Bit => {
                let new_state = self.into_bits();

                medium
                    .write(location.block, location.offset, &[new_state])
                    .await
            }
            WriteGranularity::Word => {
                let new_state = self.into_words();

                medium
                    .write(location.block, location.offset, new_state)
                    .await
            }
        }
    }
}

// TODO: representation depends on the storage medium.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ObjectLocation {
    block: usize,
    offset: usize,
}

impl ObjectLocation {
    fn new(block: usize, offset: usize) -> Self {
        Self { block, offset }
    }

    fn into_bytes<M: StorageMedium>(self) -> ([u8; 8], usize) {
        let block_bytes = self.block.to_le_bytes();
        let offset_bytes = self.offset.to_le_bytes();

        let byte_count = Self::byte_count::<M>();

        let mut bytes = [0u8; 8];
        let (block_idx_byte_slice, offset_byte_slice) =
            bytes[0..byte_count].split_at_mut(M::block_count_bytes());

        block_idx_byte_slice.copy_from_slice(&block_bytes[0..block_idx_byte_slice.len()]);
        offset_byte_slice.copy_from_slice(&offset_bytes[0..offset_byte_slice.len()]);

        (bytes, byte_count)
    }

    fn from_bytes<M: StorageMedium>(bytes: &[u8]) -> Result<Self, ()> {
        if bytes.len() != Self::byte_count::<M>() {
            return Err(());
        }

        let (block_idx_byte_slice, offset_byte_slice) = bytes.split_at(M::block_count_bytes());

        let mut block_bytes = [0u8; 4];
        block_bytes[0..block_idx_byte_slice.len()].copy_from_slice(block_idx_byte_slice);

        let mut offset_bytes = [0u8; 4];
        offset_bytes[0..offset_byte_slice.len()].copy_from_slice(offset_byte_slice);

        Ok(Self {
            block: usize::from_le_bytes(block_bytes),
            offset: usize::from_le_bytes(offset_bytes),
        })
    }

    fn byte_count<M: StorageMedium>() -> usize {
        M::block_count_bytes() + M::block_size_bytes()
    }
}

struct ObjectHeader {
    state: ObjectState,
    object_size: usize, // At most block size
}

impl ObjectHeader {
    pub async fn read<M: StorageMedium>(
        location: ObjectLocation,
        medium: &mut M,
    ) -> Result<Self, ()> {
        match M::WRITE_GRANULARITY {
            WriteGranularity::Bit => {
                let mut header_bytes = [0; 5];

                medium
                    .read(location.block, location.offset, &mut header_bytes)
                    .await?;

                let (state_bytes, size_bytes) = header_bytes.split_at(1);

                let state = ObjectState::from_bits(state_bytes[0])?;
                let object_size = usize::from_le_bytes(size_bytes.try_into().unwrap()); // TODO: M::block_size_bytes()

                Ok(Self { state, object_size })
            }

            WriteGranularity::Word => {
                let mut header_bytes = [0; 16];

                medium
                    .read(location.block, location.offset, &mut header_bytes)
                    .await?;

                let (state_bytes, size_bytes) = header_bytes.split_at(12);

                let state = ObjectState::from_words(state_bytes)?;
                let object_size = usize::from_le_bytes(size_bytes.try_into().unwrap()); // TODO: M::block_size_bytes()

                Ok(Self { state, object_size })
            }
        }
    }
}

// Object payload contains a list of object locations.
pub struct MetadataObjectHeader {
    object: ObjectHeader,
    path_hash: u32,
}

// Object payload contains a chunk of data.
pub struct DataObjectHeader {
    object: ObjectHeader,
}

pub struct ObjectWriter<'a, M: StorageMedium> {
    location: ObjectLocation,
    object: ObjectHeader,
    cursor: usize,
    medium: &'a mut M,
}

impl<'a, M: StorageMedium> ObjectWriter<'a, M> {
    pub async fn new(location: ObjectLocation, medium: &'a mut M) -> Result<Self, ()> {
        // We read back the header to ensure that the object is in a valid state.
        let object = ObjectHeader::read(location, medium).await?;

        if object.state == ObjectState::Allocated {
            // This is most likely a power loss or a bug.
            return Err(());
        }

        Ok(Self {
            location,
            object,
            cursor: 0,
            medium,
        })
    }

    pub async fn allocate(&mut self) -> Result<(), ()> {
        self.set_state(ObjectState::Allocated).await
    }

    fn data_write_offset(&self) -> usize {
        let header_size = M::object_status_bytes() // state
            + M::block_size_bytes() // max payload size
            + M::object_location_bytes(); // reserved
        self.location.offset + header_size + self.cursor
    }

    pub fn space(&self) -> usize {
        let write_offset = self.data_write_offset();

        M::BLOCK_SIZE - write_offset
    }

    pub async fn write(&mut self, data: &[u8]) -> Result<(), ()> {
        if self.object.state != ObjectState::Allocated {
            return Err(());
        }

        if self.space() < data.len() {
            // TODO once we can search for free space
            // delete current object
            // try to allocate new object with appropriate size
            // copy previous contents to new location
            return Err(());
        }

        self.medium
            .write(self.location.block, self.data_write_offset(), data)
            .await?;

        self.cursor += data.len();

        Ok(())
    }

    async fn write_size(&mut self) -> Result<(), ()> {
        ObjectOps {
            medium: self.medium,
        }
        .set_payload_size(self.location, self.cursor)
        .await
    }

    async fn set_state(&mut self, state: ObjectState) -> Result<(), ()> {
        self.object.state = state;
        ObjectOps {
            medium: self.medium,
        }
        .update_state(self.location, state)
        .await
    }

    pub async fn finalize(mut self) -> Result<(), ()> {
        if self.object.state != ObjectState::Allocated {
            return Err(());
        }

        // must be two different writes for powerloss safety
        self.write_size().await?;
        self.set_state(ObjectState::Finalized).await
    }

    pub async fn delete(mut self) -> Result<(), ()> {
        if let ObjectState::Free | ObjectState::Deleted = self.object.state {
            return Ok(());
        }

        if self.object.state == ObjectState::Allocated {
            self.write_size().await?;
        }

        self.set_state(ObjectState::Deleted).await
    }
}

pub struct ObjectReader<'a, M: StorageMedium> {
    location: ObjectLocation,
    object: ObjectHeader,
    cursor: usize,
    medium: &'a mut M,
}

impl<'a, M: StorageMedium> ObjectReader<'a, M> {
    pub async fn new(location: ObjectLocation, medium: &'a mut M) -> Result<Self, ()> {
        // We read back the header to ensure that the object is in a valid state.
        let object = ObjectHeader::read(location, medium).await?;

        if object.state != ObjectState::Finalized {
            // We can only read data from finalized objects.
            return Err(());
        }

        Ok(Self {
            location,
            object,
            cursor: 0,
            medium,
        })
    }

    pub fn remaining(&self) -> usize {
        let read_offset = self.object.object_size - self.cursor;

        M::BLOCK_SIZE - read_offset
    }

    pub fn rewind(&mut self) {
        self.cursor = 0;
    }

    pub async fn read(&mut self, buf: &mut [u8]) -> Result<usize, ()> {
        let read_offset = self.location.offset + self.cursor;
        let read_size = buf.len().min(self.remaining());

        self.medium
            .read(self.location.block, read_offset, &mut buf[0..read_size])
            .await?;

        self.cursor += read_size;

        Ok(read_size)
    }
}

pub(crate) struct ObjectOps<'a, M> {
    medium: &'a mut M,
}

impl<'a, M: StorageMedium> ObjectOps<'a, M> {
    pub async fn update_state(
        &mut self,
        location: ObjectLocation,
        state: ObjectState,
    ) -> Result<(), ()> {
        if state.is_free() {
            return Err(());
        }

        state.write(location, self.medium).await
    }

    async fn set_payload_size(
        &mut self,
        location: ObjectLocation,
        cursor: usize,
    ) -> Result<(), ()> {
        // TODO: M::block_size_bytes()
        let bytes = (cursor as u32).to_le_bytes();
        let offset = M::object_status_bytes();

        self.medium.write(location.block, offset, &bytes).await
    }
}
