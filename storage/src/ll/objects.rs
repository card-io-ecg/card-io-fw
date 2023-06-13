use core::marker::PhantomData;

use crate::{
    ll::blocks::BlockHeader,
    medium::{StorageMedium, StoragePrivate, WriteGranularity},
    StorageError,
};

// Add new ones so that ANDing two variant together results in an invalid bit pattern.
// Do not use 0xFF as it is reserved for the free state.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObjectType {
    FileMetadata = 0x8F,
    FileData = 0x8E,
}

impl ObjectType {
    fn parse(byte: u8) -> Option<Self> {
        match byte {
            v if v == Self::FileMetadata as u8 => Some(Self::FileMetadata),
            v if v == Self::FileData as u8 => Some(Self::FileData),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObjectAllocationState {
    Free,
    Allocated(ObjectType),
}

impl ObjectAllocationState {
    fn parse(byte: u8) -> Result<Self, StorageError> {
        match ObjectType::parse(byte) {
            Some(ty) => Ok(Self::Allocated(ty)),
            None if byte == 0xFF => Ok(Self::Free),
            None => {
                log::warn!("Unknown object type: 0x{byte:02X}");
                Err(StorageError::FsCorrupted)
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObjectDataState {
    Untrusted = 0xFF,
    Valid = 0xF0,
    Deleted = 0x00,
}

impl ObjectDataState {
    fn parse(byte: u8) -> Result<Self, StorageError> {
        match byte {
            v if v == Self::Untrusted as u8 => Ok(Self::Untrusted),
            v if v == Self::Valid as u8 => Ok(Self::Valid),
            v if v == Self::Deleted as u8 => Ok(Self::Deleted),
            _ => {
                log::warn!("Unknown object data state: 0x{byte:02X}");
                Err(StorageError::FsCorrupted)
            }
        }
    }

    fn parse_pair(finalized_byte: u8, deleted_byte: u8) -> Result<Self, StorageError> {
        match (finalized_byte, deleted_byte) {
            (0xFF, 0xFF) => Ok(Self::Untrusted),
            (0x00, 0xFF) => Ok(Self::Valid),
            (0x00, 0x00) => Ok(Self::Deleted),
            _ => {
                log::warn!(
                    "Unknown object data state: (0x{finalized_byte:02X}, 0x{deleted_byte:02X})"
                );
                Err(StorageError::FsCorrupted)
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CompositeObjectState {
    allocation: ObjectAllocationState,
    data: ObjectDataState,
}

impl CompositeObjectState {
    pub fn visible_state(&self) -> Result<ObjectState, StorageError> {
        let state = match (self.allocation, self.data) {
            (ObjectAllocationState::Free, ObjectDataState::Untrusted) => ObjectState::Free,
            (ObjectAllocationState::Free, _) => {
                // TODO: this shouldn't be representable
                log::warn!("Incorrect object state: {self:?}");
                return Err(StorageError::FsCorrupted);
            }
            (_, ObjectDataState::Untrusted) => ObjectState::Allocated,
            (_, ObjectDataState::Valid) => ObjectState::Finalized,
            (_, ObjectDataState::Deleted) => ObjectState::Deleted,
        };

        Ok(state)
    }

    pub fn transition(
        self,
        new_state: ObjectState,
        object_type: ObjectType,
    ) -> Result<Self, StorageError> {
        let current_state = self.visible_state()?;

        if current_state > new_state {
            // Can't go backwards in state
            return Err(StorageError::InvalidOperation);
        }

        if let ObjectAllocationState::Allocated(ty) = self.allocation {
            // Can't change allocated object type
            if ty != object_type {
                return Err(StorageError::InvalidOperation);
            }
        }

        let new_alloc_state = ObjectAllocationState::Allocated(object_type);
        let new_data_state = match new_state {
            ObjectState::Free => return Err(StorageError::InvalidOperation),
            ObjectState::Allocated => ObjectDataState::Untrusted,
            ObjectState::Finalized => ObjectDataState::Valid,
            ObjectState::Deleted => ObjectDataState::Deleted,
        };

        Ok(Self {
            allocation: new_alloc_state,
            data: new_data_state,
        })
    }

    fn byte_count<M: StorageMedium>() -> usize {
        match M::WRITE_GRANULARITY {
            WriteGranularity::Bit => 2,
            WriteGranularity::Word(w) => 3 * w,
        }
    }

    pub async fn read<M: StorageMedium>(
        medium: &mut M,
        location: ObjectLocation,
    ) -> Result<Self, StorageError> {
        log::trace!("CompositeObjectState::read({location:?})");
        let bytes_read = Self::byte_count::<M>();

        let mut buffer = [0; 12];
        assert!(bytes_read <= buffer.len());
        let buffer = &mut buffer[..bytes_read];

        medium.read(location.block, location.offset, buffer).await?;

        let allocation = ObjectAllocationState::parse(buffer[0])?;
        let data = match M::WRITE_GRANULARITY {
            WriteGranularity::Bit => ObjectDataState::parse(buffer[1])?,
            WriteGranularity::Word(w) => ObjectDataState::parse_pair(buffer[w], buffer[2 * w])?,
        };

        Ok(Self { allocation, data })
    }

    pub async fn allocate<M: StorageMedium>(
        medium: &mut M,
        location: ObjectLocation,
        object_type: ObjectType,
    ) -> Result<Self, StorageError> {
        log::trace!("CompositeObjectState::allocate({location:?}, {object_type:?})");
        let this = Self::read(medium, location).await?;
        let this = this.transition(ObjectState::Allocated, object_type)?;

        medium
            .write(location.block, location.offset, &[object_type as u8])
            .await?;

        Ok(this)
    }

    pub async fn finalize<M: StorageMedium>(
        self,
        medium: &mut M,
        location: ObjectLocation,
        object_type: ObjectType,
    ) -> Result<Self, StorageError> {
        log::trace!("CompositeObjectState::finalize({location:?}, {object_type:?})");
        let this = self.transition(ObjectState::Finalized, object_type)?;

        let (offset, byte) = match M::WRITE_GRANULARITY {
            WriteGranularity::Bit => (1, ObjectDataState::Valid as u8),
            WriteGranularity::Word(w) => (w, 0),
        };

        medium
            .write(location.block, location.offset + offset, &[byte])
            .await?;

        Ok(this)
    }

    pub async fn delete<M: StorageMedium>(
        self,
        medium: &mut M,
        location: ObjectLocation,
        object_type: ObjectType,
    ) -> Result<Self, StorageError> {
        log::trace!("CompositeObjectState::delete({location:?}, {object_type:?})");
        let this = self.transition(ObjectState::Deleted, object_type)?;

        let (offset, byte) = match M::WRITE_GRANULARITY {
            WriteGranularity::Bit => (1, ObjectDataState::Deleted as u8),
            WriteGranularity::Word(w) => (2 * w, 0),
        };

        medium
            .write(location.block, location.offset + offset, &[byte])
            .await?;

        Ok(this)
    }

    fn object_type(&self) -> Result<ObjectType, StorageError> {
        match self.allocation {
            ObjectAllocationState::Allocated(ty) => Ok(ty),
            ObjectAllocationState::Free => Err(StorageError::InvalidOperation),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Debug)]
pub enum ObjectState {
    Free,
    Allocated,
    Finalized,
    Deleted,
}

impl ObjectState {
    fn is_free(self) -> bool {
        matches!(self, ObjectState::Free)
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
    state: CompositeObjectState,
    payload_size: usize, // At most block size - header
    pub location: ObjectLocation,
}

impl ObjectHeader {
    pub fn byte_count<M: StorageMedium>() -> usize {
        CompositeObjectState::byte_count::<M>() + M::object_size_bytes()
    }

    pub async fn read<M: StorageMedium>(
        location: ObjectLocation,
        medium: &mut M,
    ) -> Result<Self, StorageError> {
        log::trace!("ObjectHeader::read({location:?})");
        let state = CompositeObjectState::read(medium, location).await?;

        let mut object_size_bytes = [0; 4];

        medium
            .read(
                location.block,
                location.offset + CompositeObjectState::byte_count::<M>(),
                &mut object_size_bytes[0..M::object_size_bytes()],
            )
            .await?;

        Ok(Self {
            state,
            payload_size: u32::from_le_bytes(object_size_bytes) as usize,
            location,
        })
    }

    pub async fn allocate<M: StorageMedium>(
        medium: &mut M,
        location: ObjectLocation,
        object_type: ObjectType,
    ) -> Result<Self, StorageError> {
        log::trace!("ObjectHeader::allocate({location:?}, {object_type:?})",);

        let state = CompositeObjectState::allocate(medium, location, object_type).await?;

        Ok(Self {
            state,
            payload_size: Self::unset_payload_size::<M>(),
            location,
        })
    }

    pub fn state(&self) -> ObjectState {
        self.state.visible_state().unwrap()
    }

    pub fn object_type(&self) -> Result<ObjectType, StorageError> {
        self.state.object_type()
    }

    pub fn unset_payload_size<M: StorageMedium>() -> usize {
        (1 << (M::object_size_bytes() * 8)) - 1
    }

    pub fn payload_size<M: StorageMedium>(&self) -> Option<usize> {
        if self.payload_size != Self::unset_payload_size::<M>() {
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

        if state == self.state.visible_state()? {
            return Ok(());
        }

        let object_type = self.object_type()?;

        self.state = match state {
            ObjectState::Finalized => {
                self.state
                    .finalize(medium, self.location, object_type)
                    .await?
            }
            ObjectState::Deleted => {
                self.state
                    .delete(medium, self.location, object_type)
                    .await?
            }
            ObjectState::Allocated => return Err(StorageError::InvalidOperation),
            ObjectState::Free => unreachable!(),
        };

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
            log::warn!("payload size already set");
            return Err(StorageError::InvalidOperation);
        }

        let bytes = size.to_le_bytes();
        let offset = M::align(CompositeObjectState::byte_count::<M>());

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
                self.location.offset + self.cursor + ObjectHeader::byte_count::<M>(),
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
    pub async fn allocate(
        location: ObjectLocation,
        object_type: ObjectType,
        medium: &mut M,
    ) -> Result<Self, StorageError> {
        Ok(Self {
            object: ObjectHeader::allocate(medium, location, object_type).await?,
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

    pub async fn write_to(
        location: ObjectLocation,
        object_type: ObjectType,
        medium: &mut M,
        data: &[u8],
    ) -> Result<usize, StorageError> {
        let mut this = Self::allocate(location, object_type, medium).await?;

        this.write(medium, data).await?;
        this.finalize(medium).await
    }

    fn data_write_offset(&self) -> usize {
        let header_size = ObjectHeader::byte_count::<M>();
        self.object.location.offset + header_size + self.cursor
    }

    pub fn space(&self) -> usize {
        M::BLOCK_SIZE - self.data_write_offset()
    }

    pub async fn write(&mut self, medium: &mut M, mut data: &[u8]) -> Result<(), StorageError> {
        if self.object.state() != ObjectState::Allocated {
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
        ObjectHeader::byte_count::<M>() + self.payload_size()
    }

    pub async fn finalize(mut self, medium: &mut M) -> Result<usize, StorageError> {
        if self.object.state() != ObjectState::Allocated {
            return Err(StorageError::InvalidOperation);
        }

        // must be two different writes for powerloss safety
        self.write_size(medium).await?;
        self.flush(medium).await?;
        self.set_state(medium, ObjectState::Finalized).await?;

        Ok(self.total_size())
    }

    pub async fn delete(mut self, medium: &mut M) -> Result<(), StorageError> {
        if let ObjectState::Free | ObjectState::Deleted = self.object.state() {
            return Ok(());
        }

        if self.object.state() == ObjectState::Allocated {
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
        log::trace!("ObjectReader::new({location:?})");

        // We read back the header to ensure that the object is in a valid state.
        let object = ObjectHeader::read(location, medium).await?;

        if object.state() != ObjectState::Finalized {
            if allow_non_finalized && object.state() != ObjectState::Free {
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
        let read_offset = self.location.offset + ObjectHeader::byte_count::<M>() + self.cursor;
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
        ObjectHeader::byte_count::<M>() + self.header.payload_size::<M>().unwrap_or(0)
    }

    pub async fn read_metadata(
        &self,
        medium: &mut M,
    ) -> Result<MetadataObjectHeader<M>, StorageError> {
        let mut path_hash_bytes = [0; 4];
        let path_hash_offset = self.location.offset + ObjectHeader::byte_count::<M>();
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
        log::trace!("ObjectInfo::read({location:?})");
        if location.offset + BlockHeader::<M>::byte_count() >= M::BLOCK_SIZE {
            return Ok(None);
        }

        let object = ObjectHeader::read(location, medium).await?;
        if object.state().is_free() {
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
