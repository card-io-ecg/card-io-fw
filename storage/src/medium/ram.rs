use crate::medium::StorageMedium;

use super::WriteGranularity;

pub struct RamStorage<const STORAGE_SIZE: usize, const BLOCK_SIZE: usize> {
    data: [u8; STORAGE_SIZE],
}

impl<const STORAGE_SIZE: usize, const BLOCK_SIZE: usize> RamStorage<STORAGE_SIZE, BLOCK_SIZE> {
    pub fn new() -> Self {
        Self {
            data: [0xFF; STORAGE_SIZE],
        }
    }

    fn offset(block: usize, offset: usize) -> usize {
        block * Self::BLOCK_SIZE + offset
    }
}

impl<const STORAGE_SIZE: usize, const BLOCK_SIZE: usize> StorageMedium
    for RamStorage<STORAGE_SIZE, BLOCK_SIZE>
{
    const BLOCK_SIZE: usize = BLOCK_SIZE;
    const BLOCK_COUNT: usize = STORAGE_SIZE / BLOCK_SIZE;
    const WRITE_GRANULARITY: WriteGranularity = WriteGranularity::Bit;

    async fn erase(&mut self, block: usize) -> Result<(), ()> {
        let offset = Self::offset(block, 0);

        self.data[offset..offset + Self::BLOCK_SIZE].fill(0xFF);

        Ok(())
    }

    async fn read(&mut self, block: usize, offset: usize, data: &mut [u8]) -> Result<(), ()> {
        assert!(offset + data.len() <= Self::BLOCK_SIZE);
        let offset = Self::offset(block, offset);

        data.copy_from_slice(&self.data[offset..offset + data.len()]);

        Ok(())
    }

    async fn write(&mut self, block: usize, offset: usize, data: &[u8]) -> Result<(), ()> {
        assert!(offset + data.len() <= Self::BLOCK_SIZE);
        let offset = Self::offset(block, offset);

        for (src, dst) in data.iter().zip(self.data[offset..].iter_mut()) {
            *dst &= *src;
        }

        Ok(())
    }
}
