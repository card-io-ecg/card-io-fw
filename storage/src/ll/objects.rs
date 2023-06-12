use core::marker::PhantomData;

use crate::{
    ll::blocks,
    medium::{StorageMedium, StoragePrivate, WriteGranularity},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ObjectState {
    Free,      // Implicit
    Allocated, // TODO: make this implicit
    Finalized,
    Deleted,
}

impl ObjectState {
    // TODO: don't assume 4 bytes per word
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
        let offset = M::align(location.offset);
        match M::WRITE_GRANULARITY {
            WriteGranularity::Bit => {
                let new_state = self.into_bits();
                medium.write(location.block, offset, &[new_state]).await
            }

            WriteGranularity::Word(_) => {
                let new_state = self.into_words();
                medium.write(location.block, offset, new_state).await
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ObjectLocation {
    pub block: usize,
    pub offset: usize,
}

impl ObjectLocation {
    fn new(block: usize, offset: usize) -> Self {
        Self { block, offset }
    }

    pub fn into_bytes<M: StorageMedium>(self) -> ([u8; 8], usize) {
        let block_bytes = self.block.to_le_bytes();
        let offset_bytes = self.offset.to_le_bytes();

        let byte_count = M::object_location_bytes();

        let mut bytes = [0u8; 8];
        let (block_idx_byte_slice, offset_byte_slice) =
            bytes[0..byte_count].split_at_mut(M::block_count_bytes());

        block_idx_byte_slice.copy_from_slice(&block_bytes[0..block_idx_byte_slice.len()]);
        offset_byte_slice.copy_from_slice(&offset_bytes[0..offset_byte_slice.len()]);

        (bytes, byte_count)
    }

    fn from_bytes<M: StorageMedium>(bytes: &[u8]) -> Result<Self, ()> {
        if bytes.len() != M::object_location_bytes() {
            return Err(());
        }

        let (block_idx_byte_slice, offset_byte_slice) = bytes.split_at(M::block_count_bytes());

        let mut block_bytes = [0u8; 4];
        block_bytes[0..block_idx_byte_slice.len()].copy_from_slice(block_idx_byte_slice);

        let mut offset_bytes = [0u8; 4];
        offset_bytes[0..offset_byte_slice.len()].copy_from_slice(offset_byte_slice);

        Ok(Self {
            block: u32::from_le_bytes(block_bytes) as usize,
            offset: u32::from_le_bytes(offset_bytes) as usize,
        })
    }

    pub async fn read_header(self, medium: &mut impl StorageMedium) -> Result<ObjectHeader, ()> {
        ObjectHeader::read(self, medium).await
    }

    pub(crate) async fn read_metadata(
        self,
        medium: &mut impl StorageMedium,
    ) -> Result<MetadataObjectHeader, ()> {
        let Some(info) = ObjectInfo::read(self, medium).await?
        else {
            return Err(());
        };

        info.read_metadata(medium).await
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ObjectHeader {
    pub state: ObjectState,
    pub object_size: usize, // At most block size
}

impl ObjectHeader {
    pub async fn read<M: StorageMedium>(
        location: ObjectLocation,
        medium: &mut M,
    ) -> Result<Self, ()> {
        let mut header_bytes = [0; 16];
        let obj_size_bytes = M::object_size_bytes();
        let status_bytes = M::object_status_bytes();
        let header_bytes = &mut header_bytes[0..obj_size_bytes + status_bytes];

        medium
            .read(location.block, location.offset, header_bytes)
            .await?;

        let (state_slice, size_slice) = header_bytes.split_at(status_bytes);

        let state = match M::WRITE_GRANULARITY {
            WriteGranularity::Bit => ObjectState::from_bits(state_slice[0])?,
            WriteGranularity::Word(_) => ObjectState::from_words(state_slice)?,
        };

        // Extend size bytes and convert to usize.
        let mut object_size_bytes = [0; 4];
        object_size_bytes[0..size_slice.len()].copy_from_slice(size_slice);
        let object_size = u32::from_le_bytes(object_size_bytes) as usize;

        Ok(Self { state, object_size })
    }
}

// Object payload contains a list of object locations.
pub struct MetadataObjectHeader {
    pub object: ObjectHeader,
    pub path_hash: u32,
    pub filename_location: ObjectLocation,
    pub location: ObjectLocation,
    cursor: usize, // Used to iterate through the list of object locations.
    parent: Option<ObjectLocation>,
}

impl MetadataObjectHeader {
    pub async fn next_object_location<M: StorageMedium>(
        &mut self,
        medium: &mut M,
    ) -> Result<Option<ObjectLocation>, ()> {
        if self.cursor > self.object.object_size {
            return Ok(None);
        }

        let mut location_bytes = [0; 8];
        let location_bytes = &mut location_bytes[0..M::object_location_bytes()];

        medium
            .read(
                self.location.block,
                self.location.offset + self.cursor,
                location_bytes,
            )
            .await?;

        self.cursor += location_bytes.len();

        ObjectLocation::from_bytes::<M>(location_bytes).map(Some)
    }

    pub async fn reset<M: StorageMedium>(&mut self) {
        // 4: path hash
        self.cursor = 4 + M::object_location_bytes();
    }
}

// Object payload contains a chunk of data.
pub struct DataObjectHeader {
    object: ObjectHeader,
}

pub struct ObjectWriter<M: StorageMedium> {
    location: ObjectLocation,
    object: ObjectHeader,
    cursor: usize,
    buffer: heapless::Vec<u8, 4>, // TODO: support larger word sizes?
    _medium: PhantomData<M>,
}

impl<M: StorageMedium> ObjectWriter<M> {
    pub async fn new(location: ObjectLocation, medium: &mut M) -> Result<Self, ()> {
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
            buffer: heapless::Vec::new(),
            _medium: PhantomData,
        })
    }

    fn fill_buffer<'d>(&mut self, data: &'d [u8]) -> Result<&'d [u8], ()> {
        // Buffering is not necessary if we can write arbitrary bits.
        match M::WRITE_GRANULARITY {
            WriteGranularity::Bit => Ok(data),
            WriteGranularity::Word(len) => {
                let copied = data.len().min(len - self.buffer.len());
                self.buffer.extend_from_slice(&data[0..copied]).unwrap();

                Ok(&data[copied..])
            }
        }
    }

    fn can_flush(&self) -> bool {
        match M::WRITE_GRANULARITY {
            WriteGranularity::Bit => false,
            WriteGranularity::Word(len) => self.buffer.len() == len,
        }
    }

    async fn flush(&mut self, medium: &mut M) -> Result<(), ()> {
        // Buffering is not necessary if we can write arbitrary bits.
        if M::WRITE_GRANULARITY == WriteGranularity::Bit {
            return Ok(());
        }

        if !self.buffer.is_empty() {
            let offset = self.data_write_offset();
            medium
                .write(self.location.block, offset, &self.buffer)
                .await?;

            self.buffer.clear();
        }

        Ok(())
    }

    pub async fn allocate(&mut self, medium: &mut M) -> Result<(), ()> {
        self.set_state(medium, ObjectState::Allocated).await
    }

    pub async fn write_to(
        location: ObjectLocation,
        medium: &mut M,
        data: &[u8],
    ) -> Result<usize, ()> {
        let mut this = Self::new(location, medium).await?;

        this.allocate(medium).await?;
        this.write(medium, data).await?;
        this.finalize(medium).await
    }

    fn data_write_offset(&self) -> usize {
        let header_size = M::object_header_bytes();
        self.location.offset + header_size + self.cursor
    }

    pub fn space(&self) -> usize {
        M::BLOCK_SIZE - self.data_write_offset()
    }

    pub async fn write(&mut self, medium: &mut M, mut data: &[u8]) -> Result<(), ()> {
        if self.object.state != ObjectState::Allocated {
            return Err(());
        }

        let len = data.len();

        if self.space() < len {
            // TODO once we can search for free space
            // delete current object
            // try to allocate new object with appropriate size
            // copy previous contents to new location
            return Err(());
        }

        if !self.buffer.is_empty() {
            data = self.fill_buffer(data)?;
            if self.can_flush() {
                self.flush(medium).await?;
            }
        }

        let remaining = data.len() % M::WRITE_GRANULARITY.width();
        let aligned_bytes = len - remaining;
        medium
            .write(
                self.location.block,
                self.data_write_offset(),
                &data[0..aligned_bytes],
            )
            .await?;

        data = self.fill_buffer(&data[aligned_bytes..])?;

        debug_assert!(data.is_empty());

        self.cursor += len;

        Ok(())
    }

    async fn write_size(&mut self, medium: &mut M) -> Result<(), ()> {
        ObjectOps { medium }
            .set_payload_size(self.location, self.cursor)
            .await
    }

    async fn set_state(&mut self, medium: &mut M, state: ObjectState) -> Result<(), ()> {
        self.object.state = state;
        ObjectOps { medium }
            .update_state(self.location, state)
            .await
    }

    pub async fn finalize(mut self, medium: &mut M) -> Result<usize, ()> {
        if self.object.state != ObjectState::Allocated {
            return Err(());
        }

        // must be two different writes for powerloss safety
        self.write_size(medium).await?;
        self.flush(medium).await?;
        self.set_state(medium, ObjectState::Finalized).await?;

        Ok(M::object_header_bytes() + self.cursor)
    }

    pub async fn delete(mut self, medium: &mut M) -> Result<(), ()> {
        if let ObjectState::Free | ObjectState::Deleted = self.object.state {
            return Ok(());
        }

        if self.object.state == ObjectState::Allocated {
            self.write_size(medium).await?;
        }

        self.flush(medium).await?;
        self.set_state(medium, ObjectState::Deleted).await
    }
}

pub struct ObjectReader<M: StorageMedium> {
    location: ObjectLocation,
    object: ObjectHeader,
    cursor: usize,
    _medium: PhantomData<M>,
}

impl<M: StorageMedium> ObjectReader<M> {
    pub async fn new(location: ObjectLocation, medium: &mut M) -> Result<Self, ()> {
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
            _medium: PhantomData,
        })
    }

    pub fn len(&self) -> usize {
        self.object.object_size
    }

    pub fn remaining(&self) -> usize {
        let read_offset = self.object.object_size - self.cursor;

        M::BLOCK_SIZE - read_offset
    }

    pub fn rewind(&mut self) {
        self.cursor = 0;
    }

    pub async fn read(&mut self, medium: &mut M, buf: &mut [u8]) -> Result<usize, ()> {
        let read_offset = self.location.offset + M::object_header_bytes() + self.cursor;
        let read_size = buf.len().min(self.remaining());

        medium
            .read(self.location.block, read_offset, &mut buf[0..read_size])
            .await?;

        self.cursor += read_size;

        Ok(read_size)
    }
}

pub struct ObjectInfo<M: StorageMedium> {
    pub location: ObjectLocation,
    pub header: ObjectHeader,
    _medium: PhantomData<M>,
}

impl<M: StorageMedium> ObjectInfo<M> {
    pub fn total_size(&self) -> usize {
        self.header.object_size + M::object_header_bytes()
    }

    pub async fn read_metadata(&self, medium: &mut M) -> Result<MetadataObjectHeader, ()> {
        let mut path_hash_bytes = [0; 4];
        let path_hash_offset = self.location.offset + M::object_header_bytes();
        medium
            .read(self.location.block, path_hash_offset, &mut path_hash_bytes)
            .await?;

        let mut location_bytes = [0; 8];
        let location_bytes = &mut location_bytes[0..M::object_location_bytes()];

        medium
            .read(self.location.block, path_hash_offset + 4, location_bytes)
            .await?;

        Ok(MetadataObjectHeader {
            object: self.header,
            path_hash: u32::from_le_bytes(path_hash_bytes),
            filename_location: ObjectLocation::from_bytes::<M>(location_bytes)?,
            location: self.location,
            cursor: 4 + M::object_location_bytes(), // skip path hash and filename
            parent: None,
        })
    }

    async fn read(location: ObjectLocation, medium: &mut M) -> Result<Option<Self>, ()> {
        if location.offset + blocks::HEADER_BYTES >= M::BLOCK_SIZE {
            return Ok(None);
        }

        let object = ObjectHeader::read(location, medium).await?;
        if object.state.is_free() {
            return Ok(None);
        }

        let info = ObjectInfo {
            location,
            header: object,
            _medium: PhantomData,
        };

        Ok(Some(info))
    }
}

pub struct ObjectIterator {
    location: ObjectLocation,
}

impl ObjectIterator {
    pub fn new(block: usize) -> Self {
        Self {
            location: ObjectLocation {
                block,
                offset: blocks::HEADER_BYTES,
            },
        }
    }

    pub async fn next<M: StorageMedium>(
        &mut self,
        medium: &mut M,
    ) -> Result<Option<ObjectInfo<M>>, ()> {
        let info = ObjectInfo::read(self.location, medium).await?;
        if let Some(info) = info.as_ref() {
            self.location.offset += info.total_size();
        }
        Ok(info)
    }

    pub fn current_offset(&self) -> usize {
        self.location.offset
    }
}

pub(crate) struct ObjectOps<'a, M> {
    pub medium: &'a mut M,
}

impl<'a, M: StorageMedium> ObjectOps<'a, M> {
    pub(crate) fn new(medium: &'a mut M) -> Self {
        Self { medium }
    }

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

    pub async fn set_payload_size(
        &mut self,
        location: ObjectLocation,
        cursor: usize,
    ) -> Result<(), ()> {
        let bytes = cursor.to_le_bytes();
        let offset = M::align(M::object_status_bytes());

        self.medium
            .write(
                location.block,
                location.offset + offset,
                &bytes[0..M::object_size_bytes()],
            )
            .await
    }
}
