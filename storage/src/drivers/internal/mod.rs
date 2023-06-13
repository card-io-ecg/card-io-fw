use core::ptr::{addr_of, addr_of_mut};

use crate::{
    medium::{StorageMedium, WriteGranularity},
    StorageError,
};

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

pub struct InternalFlash<P: InternalPartition> {
    unlocked: bool,
    _partition: P,
}

impl<P: InternalPartition> InternalFlash<P> {
    const OFFSET_ALIGNED: () = assert!(P::OFFSET % implem::BLOCK_SIZE == 0);
    const SIZE_ALIGNED: () = assert!(P::SIZE % implem::BLOCK_SIZE == 0);

    pub fn new(partition: P) -> Self {
        let _used = Self::OFFSET_ALIGNED;
        let _used = Self::SIZE_ALIGNED;

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

    fn read(offset: u32, ptr: *mut u32, len: u32) -> Result<(), StorageError> {
        if implem::esp_rom_spiflash_read(offset, ptr, len) == 0 {
            Ok(())
        } else {
            Err(StorageError::Io)
        }
    }

    fn write(offset: u32, ptr: *const u32, len: u32) -> Result<(), StorageError> {
        if implem::esp_rom_spiflash_write(offset, ptr, len) == 0
            && implem::esp_rom_spiflash_wait_idle(implem::CHIP_PTR) == 0
        {
            Ok(())
        } else {
            Err(StorageError::Io)
        }
    }
}

impl<P: InternalPartition> StorageMedium for InternalFlash<P> {
    const BLOCK_SIZE: usize = implem::BLOCK_SIZE;
    const BLOCK_COUNT: usize = P::SIZE / implem::BLOCK_SIZE;
    const WRITE_GRANULARITY: WriteGranularity = implem::WRITE_GRANULARITY;

    async fn erase(&mut self, block: usize) -> Result<(), StorageError> {
        self.unlock()?;

        let block_offset = P::OFFSET / Self::BLOCK_SIZE;

        if implem::esp_rom_spiflash_erase_block((block_offset + block) as u32) == 0
            && implem::esp_rom_spiflash_wait_idle(implem::CHIP_PTR) == 0
        {
            Ok(())
        } else {
            Err(StorageError::Io)
        }
    }

    async fn read(
        &mut self,
        block: usize,
        offset: usize,
        mut data: &mut [u8],
    ) -> Result<(), StorageError> {
        let mut offset = block * Self::BLOCK_SIZE + offset;

        if offset % 4 != 0 {
            let align_amt = offset % 4;
            let aligned_offset = offset - align_amt;
            let unaligned_count = 4 - align_amt;

            let mut buffer = 0u32;
            Self::read(aligned_offset as u32, addr_of_mut!(buffer), 4)?;

            let (unaligned_data, aligned_data) = data.split_at_mut(unaligned_count);
            unaligned_data.copy_from_slice(&buffer.to_le_bytes()[offset % 4..]);
            offset = offset + unaligned_count;
            data = aligned_data;
        }

        // offset is aligned at this point, data may not be
        let shift_after_read = data.as_ptr() as usize % 4;
        let aligned_buffer = &mut data[shift_after_read..];
        let aligned_len = aligned_buffer.len() - aligned_buffer.len() % 4;
        let mut aligned_buffer = &mut aligned_buffer[0..aligned_len];

        // read aligned data, one (maybe partial) page at a time
        while !aligned_buffer.is_empty() {
            let bytes_in_page = implem::PAGE_SIZE - offset % implem::PAGE_SIZE;
            let bytes_to_read = bytes_in_page.min(aligned_buffer.len());
            Self::read(
                offset as u32,
                aligned_buffer.as_mut_ptr().cast(),
                bytes_to_read as u32,
            )?;
            offset += bytes_to_read;
            aligned_buffer = &mut aligned_buffer[bytes_to_read..];
        }

        // align data in the slice, if we have to
        if shift_after_read != 0 {
            data.copy_within(shift_after_read..shift_after_read + aligned_len, 0);
            data = &mut data[aligned_len..];
        }

        // read unaligned end of data - in a loop because we might be at a page boundary
        while data.len() != 0 {
            let mut buffer = 0u32;
            Self::read(offset as u32, addr_of_mut!(buffer), 4)?;

            let copy_len = data.len().min(4);
            data[..copy_len].copy_from_slice(&buffer.to_le_bytes()[..copy_len]);
            data = &mut data[copy_len..];
        }

        Ok(())
    }

    async fn write(
        &mut self,
        block: usize,
        offset: usize,
        mut data: &[u8],
    ) -> Result<(), StorageError> {
        self.unlock()?;
        let mut offset = block * Self::BLOCK_SIZE + offset;

        if offset % 4 != 0 {
            let align_amt = offset % 4;
            let aligned_offset = offset - align_amt;
            let unaligned_count = (4 - align_amt).min(data.len());

            let mut buffer = [0xFF; 4];

            let (unaligned_data, aligned_data) = data.split_at(unaligned_count);
            buffer[align_amt..align_amt + unaligned_count].copy_from_slice(unaligned_data);

            let buffer = u32::from_le_bytes(buffer);
            Self::write(aligned_offset as u32, addr_of!(buffer), 4)?;

            offset = offset + unaligned_count;
            data = aligned_data;
        };

        if data.as_ptr() as u32 % 4 == 0 {
            while data.len() >= 4 {
                let bytes_in_page = implem::PAGE_SIZE - offset % implem::PAGE_SIZE;
                let bytes_to_write = bytes_in_page.min(data.len());
                let bytes_to_write = bytes_to_write - bytes_to_write % 4;

                Self::write(offset as u32, addr_of!(data).cast(), bytes_to_write as u32)?;
                offset += bytes_to_write;
                data = &data[bytes_to_write..];
            }

            if !data.is_empty() {
                let mut buffer = [0xFF; 4];
                buffer[..data.len()].copy_from_slice(data);
                let buffer = u32::from_le_bytes(buffer);

                Self::write(offset as u32, addr_of!(buffer), 4)?;
            }
        } else {
            #[repr(align(4))]
            struct Buffer {
                data: [u8; implem::PAGE_SIZE],
            }
            let mut buffer = Buffer {
                data: [0; implem::PAGE_SIZE],
            };

            while !data.is_empty() {
                let bytes_in_page = implem::PAGE_SIZE - offset % implem::PAGE_SIZE;
                let mut bytes_to_write = bytes_in_page.min(data.len());
                buffer.data[..bytes_to_write].copy_from_slice(&data[..bytes_to_write]);

                if bytes_to_write % 4 != 0 {
                    let pad = 4 - bytes_to_write % 4;
                    buffer.data[bytes_to_write..bytes_to_write + pad].fill(0xFF);
                    bytes_to_write += pad;
                }

                Self::write(
                    offset as u32,
                    addr_of!(buffer.data).cast(),
                    bytes_to_write as u32,
                )?;
                offset += bytes_to_write;
                data = &data[bytes_to_write..];
            }
        }

        Ok(())
    }
}
