use crate::{drivers::aligned::AlignedStorage, medium::WriteGranularity, StorageError};

#[cfg_attr(feature = "esp32s3", path = "esp32s3.rs")]
pub mod implem;

#[cfg(feature = "critical-section")]
#[inline(always)]
#[link_section = ".rwtext"]
fn maybe_with_critical_section<R>(f: impl FnOnce() -> R) -> R {
    return critical_section::with(|_| f());

    #[cfg(not(feature = "critical-section"))]
    f()
}

#[cfg(not(feature = "critical-section"))]
#[inline(always)]
#[allow(unused)]
fn maybe_with_critical_section<R>(f: impl FnOnce() -> R) -> R {
    f()
}

pub trait InternalPartition {
    const OFFSET: usize;
    const SIZE: usize;
}

pub struct InternalDriver<P: InternalPartition> {
    unlocked: bool,
    _partition: P,
}

impl<P: InternalPartition> InternalDriver<P> {
    pub fn new(partition: P) -> Self {
        Self {
            unlocked: false,
            _partition: partition,
        }
    }

    fn unlock(&mut self) -> Result<(), StorageError> {
        if !self.unlocked {
            if implem::esp_rom_spiflash_unlock() != 0 {
                return Err(StorageError::Io);
            }
            self.unlocked = true;
        }

        Ok(())
    }

    async fn wait_idle(&mut self) -> Result<(), StorageError> {
        const SR_WIP: u32 = 1 << 0;

        let mut status = 0x00;
        loop {
            if implem::esp_rom_spiflash_read_status(implem::CHIP_PTR, &mut status) != 0 {
                return Err(StorageError::Io);
            }
            if status & SR_WIP == 0 {
                return Ok(());
            }
            embassy_futures::yield_now().await;
        }
    }
}

impl<P: InternalPartition> AlignedStorage for InternalDriver<P> {
    const BLOCK_COUNT: usize = P::SIZE / implem::BLOCK_SIZE;
    const BLOCK_SIZE: usize = implem::BLOCK_SIZE;
    const PAGE_SIZE: usize = implem::PAGE_SIZE;
    const WRITE_GRANULARITY: WriteGranularity = implem::WRITE_GRANULARITY;

    async fn erase(&mut self, block: usize) -> Result<(), StorageError> {
        self.unlock()?;

        let offset = P::OFFSET / Self::BLOCK_SIZE;
        let block = offset + block;

        if implem::esp_rom_spiflash_erase_block(block as u32) == 0 {
            self.wait_idle().await
        } else {
            Err(StorageError::Io)
        }
    }

    async fn read_aligned(
        &mut self,
        block: usize,
        offset: usize,
        data: &mut [u8],
    ) -> Result<(), StorageError> {
        let len = data.len() as u32;
        let ptr = data.as_mut_ptr().cast();

        let offset = P::OFFSET + block * Self::BLOCK_SIZE + offset;

        if implem::esp_rom_spiflash_read(offset as u32, ptr, len) == 0 {
            Ok(())
        } else {
            Err(StorageError::Io)
        }
    }

    async fn write_aligned(
        &mut self,
        block: usize,
        offset: usize,
        data: &[u8],
    ) -> Result<(), StorageError> {
        let len = data.len() as u32;
        let ptr = data.as_ptr().cast();

        let offset = P::OFFSET + block * Self::BLOCK_SIZE + offset;

        if implem::esp_rom_spiflash_write(offset as u32, ptr, len) == 0 {
            self.wait_idle().await
        } else {
            Err(StorageError::Io)
        }
    }
}
