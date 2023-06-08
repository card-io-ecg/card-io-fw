use crate::medium::{StorageMedium, StoragePrivate};

#[derive(Clone, Copy, PartialEq, Eq)]
enum ObjectState {
    Free, // Implicit
    Allocated,
    Finalized,
    Deleted,
}

impl ObjectState {
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

    fn from_bits(self, bits: u8) -> Result<Self, ()> {
        match bits {
            0xFF => Ok(ObjectState::Free),
            0xFE => Ok(ObjectState::Allocated),
            0xFC => Ok(ObjectState::Finalized),
            0x00 => Ok(ObjectState::Deleted),
            _ => Err(()),
        }
    }

    fn into_words(self) -> [u32; 3] {
        match self {
            Self::Free => [0xFFFFFFFF, 0xFFFFFFFF, 0xFFFFFFFF],
            Self::Allocated => [0x00000000, 0xFFFFFFFF, 0xFFFFFFFF],
            Self::Finalized => [0x00000000, 0x00000000, 0xFFFFFFFF],
            Self::Deleted => [0x00000000, 0x00000000, 0x00000000],
        }
    }

    fn from_words(self, words: [u32; 3]) -> Result<Self, ()> {
        match words {
            [0xFFFFFFFF, 0xFFFFFFFF, 0xFFFFFFFF] => Ok(Self::Free),
            [0x00000000, 0xFFFFFFFF, 0xFFFFFFFF] => Ok(Self::Allocated),
            [0x00000000, 0x00000000, 0xFFFFFFFF] => Ok(Self::Finalized),
            [0x00000000, 0x00000000, 0x00000000] => Ok(Self::Deleted),
            _ => Err(()),
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
    object_size: u32, // At most block size
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
