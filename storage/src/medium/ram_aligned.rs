use crate::{drivers::aligned::AlignedStorage, StorageError};

use super::WriteGranularity;

pub struct AlignedNorRamStorage<const STORAGE_SIZE: usize, const BLOCK_SIZE: usize> {
    pub(crate) data: [u32; STORAGE_SIZE],
}

impl<const STORAGE_SIZE: usize, const BLOCK_SIZE: usize> Default
    for AlignedNorRamStorage<STORAGE_SIZE, BLOCK_SIZE>
{
    fn default() -> Self {
        Self::new()
    }
}

impl<const STORAGE_SIZE: usize, const BLOCK_SIZE: usize>
    AlignedNorRamStorage<STORAGE_SIZE, BLOCK_SIZE>
{
    pub const fn new() -> Self {
        Self {
            data: [u32::MAX; STORAGE_SIZE],
        }
    }

    fn offset(block: usize, offset: usize) -> usize {
        block * Self::BLOCK_SIZE + offset
    }

    #[cfg(test)]
    pub fn debug_print(&self) {
        for blk in 0..Self::BLOCK_COUNT {
            print!("{blk:02X}:");

            for word in 0..Self::BLOCK_SIZE {
                for byte in self.data[Self::offset(blk, word)].to_le_bytes() {
                    print!(" {byte:02X}");
                }
            }

            println!();
        }
    }
}

impl<const STORAGE_SIZE: usize, const BLOCK_SIZE: usize> AlignedStorage
    for AlignedNorRamStorage<STORAGE_SIZE, BLOCK_SIZE>
{
    const BLOCK_SIZE: usize = BLOCK_SIZE;
    const BLOCK_COUNT: usize = STORAGE_SIZE / BLOCK_SIZE;
    const WRITE_GRANULARITY: WriteGranularity = WriteGranularity::Bit;
    const PAGE_SIZE: usize = BLOCK_SIZE;

    async fn erase(&mut self, block: usize) -> Result<(), StorageError> {
        let offset = Self::offset(block, 0);

        self.data[offset..offset + Self::BLOCK_SIZE].fill(u32::MAX);

        Ok(())
    }

    async fn read_aligned(
        &mut self,
        block: usize,
        offset: usize,
        data: &mut [u8],
    ) -> Result<(), StorageError> {
        assert!(
            offset + data.len() <= Self::BLOCK_SIZE * 4,
            "{offset} + {} <= {}",
            data.len(),
            Self::BLOCK_SIZE
        );
        assert!(offset % 4 == 0, "offset must be aligned to 4 bytes");
        assert!(
            data.len() % 4 == 0,
            "data length must be aligned to 4 bytes"
        );
        assert!(
            data.as_ptr() as usize % 4 == 0,
            "data must be aligned to 4 bytes"
        );

        let word_offset = offset / 4;
        let index = block * Self::BLOCK_SIZE + word_offset;

        for idx in 0..data.len() / 4 {
            let word = self.data[index + idx];
            let bytes = word.to_le_bytes();
            data[idx * 4..idx * 4 + 4].copy_from_slice(&bytes);
        }

        Ok(())
    }

    async fn write_aligned(
        &mut self,
        block: usize,
        offset: usize,
        data: &[u8],
    ) -> Result<(), StorageError> {
        assert!(
            offset + data.len() <= Self::BLOCK_SIZE * 4,
            "{offset} + {} <= {}",
            data.len(),
            Self::BLOCK_SIZE
        );
        assert!(offset % 4 == 0, "offset must be aligned to 4 bytes");
        assert!(
            data.len() % 4 == 0,
            "data length must be aligned to 4 bytes"
        );
        assert!(
            data.as_ptr() as usize % 4 == 0,
            "data must be aligned to 4 bytes"
        );

        let word_offset = offset / 4;
        let index = block * Self::BLOCK_SIZE + word_offset;

        for idx in 0..data.len() / 4 {
            let bytes = data[idx * 4..idx * 4 + 4].try_into().unwrap();
            let word = u32::from_le_bytes(bytes);
            self.data[index + idx] &= word;
        }

        Ok(())
    }
}
