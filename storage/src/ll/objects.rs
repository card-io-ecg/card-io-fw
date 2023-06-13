use core::marker::PhantomData;

use crate::{
    ll::blocks::BlockHeader,
    medium::{StorageMedium, StoragePrivate, WriteGranularity},
    StorageError,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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

    fn into_bits(self) -> u8 {
        match self {
            ObjectState::Free => 0xFF,
            ObjectState::Allocated => 0xFE,
            ObjectState::Finalized => 0xFC,
            ObjectState::Deleted => 0x00,
        }
    }

    fn from_bits(bits: u8) -> Result<Self, StorageError> {
        match bits {
            0xFF => Ok(ObjectState::Free),
            0xFE => Ok(ObjectState::Allocated),
            0xFC => Ok(ObjectState::Finalized),
            0x00 => Ok(ObjectState::Deleted),
            _ => Err(StorageError::FsCorrupted),
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

    fn from_words(words: &[u8]) -> Result<Self, StorageError> {
        match words {
            Self::FREE_WORDS => Ok(Self::Free),
            Self::ALLOCATED_WORDS => Ok(Self::Allocated),
            Self::FINALIZED_WORDS => Ok(Self::Finalized),
            Self::DELETED_WORDS => Ok(Self::Deleted),
            _ => Err(StorageError::FsCorrupted),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ObjectLocation {
    pub block: usize,
    pub offset: usize,
}

impl ObjectLocation {
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

    fn from_bytes<M: StorageMedium>(bytes: &[u8]) -> Self {
        debug_assert_eq!(bytes.len(), M::object_location_bytes());

        let (block_idx_byte_slice, offset_byte_slice) = bytes.split_at(M::block_count_bytes());

        let mut block_bytes = [0u8; 4];
        block_bytes[0..block_idx_byte_slice.len()].copy_from_slice(block_idx_byte_slice);

        let mut offset_bytes = [0u8; 4];
        offset_bytes[0..offset_byte_slice.len()].copy_from_slice(offset_byte_slice);

        Self {
            block: u32::from_le_bytes(block_bytes) as usize,
            offset: u32::from_le_bytes(offset_bytes) as usize,
        }
    }

    pub async fn read_header(
        self,
        medium: &mut impl StorageMedium,
    ) -> Result<ObjectHeader, StorageError> {
        ObjectHeader::read(self, medium).await
    }

    pub(crate) async fn read_metadata<M: StorageMedium>(
        self,
        medium: &mut M,
    ) -> Result<MetadataObjectHeader<M>, StorageError> {
        if let Some(info) = ObjectInfo::read(self, medium).await? {
            info.read_metadata(medium).await
        } else {
            Err(StorageError::FsCorrupted)
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ObjectHeader {
    state: ObjectState,
    payload_size: usize, // At most block size - header
    pub location: ObjectLocation,
}

impl ObjectHeader {
    pub async fn read<M: StorageMedium>(
        location: ObjectLocation,
        medium: &mut M,
    ) -> Result<Self, StorageError> {
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

        Ok(Self {
            state,
            payload_size: object_size,
            location,
        })
    }

    pub fn state(&self) -> ObjectState {
        self.state
    }

    pub fn payload_size<M: StorageMedium>(&self) -> Option<usize> {
        let unset_object_size = (1 << (M::object_size_bytes() * 8)) - 1;
        if self.payload_size != unset_object_size {
            Some(self.payload_size)
        } else {
            None
        }
    }

    pub async fn update_state<M: StorageMedium>(
        &mut self,
        medium: &mut M,
        state: ObjectState,
    ) -> Result<(), StorageError> {
        debug_assert!(!state.is_free());

        log::trace!("ObjectHeader::update_state({:?}, {state:?})", self.location);

        let offset = M::align(self.location.offset);
        match M::WRITE_GRANULARITY {
            WriteGranularity::Bit => {
                let new_state = state.into_bits();
                medium
                    .write(self.location.block, offset, &[new_state])
                    .await?;
            }

            WriteGranularity::Word(_) => {
                let new_state = state.into_words();
                medium.write(self.location.block, offset, new_state).await?;
            }
        }

        self.state = state;
        Ok(())
    }

    pub async fn set_payload_size<M: StorageMedium>(
        &mut self,
        medium: &mut M,
        size: usize,
    ) -> Result<(), StorageError> {
        log::trace!(
            "ObjectHeader::set_payload_size({:?}, {size})",
            self.location
        );

        if self.payload_size::<M>().is_some() {
            return Err(StorageError::InvalidOperation);
        }

        let bytes = size.to_le_bytes();
        let offset = M::align(M::object_status_bytes());

        medium
            .write(
                self.location.block,
                self.location.offset + offset,
                &bytes[0..M::object_size_bytes()],
            )
            .await?;
        self.payload_size = size;
        Ok(())
    }
}

// Object payload contains a list of object locations.
pub struct MetadataObjectHeader<M: StorageMedium> {
    pub object: ObjectHeader,
    pub path_hash: u32,
    pub filename_location: ObjectLocation,
    pub location: ObjectLocation,
    cursor: usize, // Used to iterate through the list of object locations.
    _parent: Option<ObjectLocation>,
    _medium: PhantomData<M>,
}

impl<M: StorageMedium> MetadataObjectHeader<M> {
    pub async fn next_object_location(
        &mut self,
        medium: &mut M,
    ) -> Result<Option<ObjectLocation>, StorageError> {
        if self.cursor >= self.object.payload_size {
            return Ok(None);
        }

        let mut location_bytes = [0; 8];
        let location_bytes = &mut location_bytes[0..M::object_location_bytes()];

        medium
            .read(
                self.location.block,
                self.location.offset + self.cursor + M::object_header_bytes(),
                location_bytes,
            )
            .await?;

        self.cursor += location_bytes.len();

        Ok(Some(ObjectLocation::from_bytes::<M>(location_bytes)))
    }

    pub async fn reset(&mut self) {
        // 4: path hash
        self.cursor = 4 + M::object_location_bytes();
    }
}

pub struct ObjectWriter<M: StorageMedium> {
    object: ObjectHeader,
    cursor: usize,
    buffer: heapless::Vec<u8, 4>, // TODO: support larger word sizes?
    _medium: PhantomData<M>,
}

impl<M: StorageMedium> ObjectWriter<M> {
    pub async fn new(location: ObjectLocation, medium: &mut M) -> Result<Self, StorageError> {
        // We read back the header to ensure that the object is in a valid state.
        let object = ObjectHeader::read(location, medium).await?;

        if object.state == ObjectState::Allocated {
            // This is most likely a power loss or a bug.
            return Err(StorageError::FsCorrupted);
        }

        Ok(Self {
            object,
            cursor: 0,
            buffer: heapless::Vec::new(),
            _medium: PhantomData,
        })
    }

    fn fill_buffer<'d>(&mut self, data: &'d [u8]) -> &'d [u8] {
        // Buffering is not necessary if we can write arbitrary bits.
        match M::WRITE_GRANULARITY {
            WriteGranularity::Bit => data,
            WriteGranularity::Word(len) => {
                let copied = data.len().min(len - self.buffer.len());
                self.buffer.extend_from_slice(&data[0..copied]).unwrap();

                &data[copied..]
            }
        }
    }

    fn can_flush(&self) -> bool {
        match M::WRITE_GRANULARITY {
            WriteGranularity::Bit => false,
            WriteGranularity::Word(len) => self.buffer.len() == len,
        }
    }

    async fn flush(&mut self, medium: &mut M) -> Result<(), StorageError> {
        // Buffering is not necessary if we can write arbitrary bits.
        if M::WRITE_GRANULARITY == WriteGranularity::Bit {
            return Ok(());
        }

        if !self.buffer.is_empty() {
            let offset = self.data_write_offset();
            medium
                .write(self.object.location.block, offset, &self.buffer)
                .await?;

            self.buffer.clear();
        }

        Ok(())
    }

    pub async fn allocate(&mut self, medium: &mut M) -> Result<(), StorageError> {
        self.set_state(medium, ObjectState::Allocated).await
    }

    pub async fn write_to(
        location: ObjectLocation,
        medium: &mut M,
        data: &[u8],
    ) -> Result<usize, StorageError> {
        let mut this = Self::new(location, medium).await?;

        this.allocate(medium).await?;
        this.write(medium, data).await?;
        this.finalize(medium).await
    }

    fn data_write_offset(&self) -> usize {
        let header_size = M::object_header_bytes();
        self.object.location.offset + header_size + self.cursor
    }

    pub fn space(&self) -> usize {
        M::BLOCK_SIZE - self.data_write_offset()
    }

    pub async fn write(&mut self, medium: &mut M, mut data: &[u8]) -> Result<(), StorageError> {
        if self.object.state != ObjectState::Allocated {
            return Err(StorageError::InvalidOperation);
        }

        let len = data.len();

        if self.space() < len {
            return Err(StorageError::InsufficientSpace);
        }

        if !self.buffer.is_empty() {
            data = self.fill_buffer(data);
            if self.can_flush() {
                self.flush(medium).await?;
            }
        }

        let remaining = data.len() % M::WRITE_GRANULARITY.width();
        let aligned_bytes = len - remaining;
        medium
            .write(
                self.object.location.block,
                self.data_write_offset(),
                &data[0..aligned_bytes],
            )
            .await?;

        data = self.fill_buffer(&data[aligned_bytes..]);

        debug_assert!(data.is_empty());

        self.cursor += len;

        Ok(())
    }

    async fn write_size(&mut self, medium: &mut M) -> Result<(), StorageError> {
        self.object.set_payload_size(medium, self.cursor).await
    }

    async fn set_state(&mut self, medium: &mut M, state: ObjectState) -> Result<(), StorageError> {
        self.object.update_state(medium, state).await
    }

    pub fn payload_size(&self) -> usize {
        self.cursor
    }

    pub fn total_size(&self) -> usize {
        M::object_header_bytes() + self.payload_size()
    }

    pub async fn finalize(mut self, medium: &mut M) -> Result<usize, StorageError> {
        if self.object.state != ObjectState::Allocated {
            return Err(StorageError::InvalidOperation);
        }

        // must be two different writes for powerloss safety
        self.write_size(medium).await?;
        self.flush(medium).await?;
        self.set_state(medium, ObjectState::Finalized).await?;

        Ok(self.total_size())
    }

    pub async fn delete(mut self, medium: &mut M) -> Result<(), StorageError> {
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
    pub async fn new(
        location: ObjectLocation,
        medium: &mut M,
        allow_non_finalized: bool,
    ) -> Result<Self, StorageError> {
        // We read back the header to ensure that the object is in a valid state.
        let object = ObjectHeader::read(location, medium).await?;

        if object.state != ObjectState::Finalized {
            if allow_non_finalized && object.state != ObjectState::Free {
                // We can read data from unfinalized/deleted objects if the caller allows it.
            } else {
                // We can only read data from finalized objects.
                return Err(StorageError::FsCorrupted);
            }
        }

        Ok(Self {
            location,
            object,
            cursor: 0,
            _medium: PhantomData,
        })
    }

    pub fn len(&self) -> usize {
        self.object.payload_size
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn remaining(&self) -> usize {
        self.len() - self.cursor
    }

    pub fn rewind(&mut self) {
        self.cursor = 0;
    }

    pub async fn read(&mut self, medium: &mut M, buf: &mut [u8]) -> Result<usize, StorageError> {
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
    pub fn state(&self) -> ObjectState {
        self.header.state()
    }

    pub fn total_size(&self) -> usize {
        self.header.payload_size + M::object_header_bytes()
    }

    pub async fn read_metadata(
        &self,
        medium: &mut M,
    ) -> Result<MetadataObjectHeader<M>, StorageError> {
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
            filename_location: ObjectLocation::from_bytes::<M>(location_bytes),
            location: self.location,
            cursor: 4 + M::object_location_bytes(), // skip path hash and filename
            _parent: None,
            _medium: PhantomData,
        })
    }

    async fn read(location: ObjectLocation, medium: &mut M) -> Result<Option<Self>, StorageError> {
        if location.offset + BlockHeader::<M>::byte_count() >= M::BLOCK_SIZE {
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
    pub fn new<M: StorageMedium>(block: usize) -> Self {
        Self {
            location: ObjectLocation {
                block,
                offset: BlockHeader::<M>::byte_count(),
            },
        }
    }

    pub async fn next<M: StorageMedium>(
        &mut self,
        medium: &mut M,
    ) -> Result<Option<ObjectInfo<M>>, StorageError> {
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
