use crate::{
    medium::{StorageMedium, WriteGranularity},
    StorageError,
};

#[repr(align(4))]
struct AlignedBuffer<const N: usize> {
    data: [u8; N],
}

impl<const N: usize> AlignedBuffer<N> {
    fn new() -> Self {
        Self { data: [0; N] }
    }
}

impl AlignedBuffer<4> {
    fn from_u32(data: u32) -> Self {
        Self {
            data: data.to_le_bytes(),
        }
    }
}

pub trait AlignedStorage {
    const BLOCK_SIZE: usize;
    const BLOCK_COUNT: usize;
    const WRITE_GRANULARITY: WriteGranularity;
    const PAGE_SIZE: usize;

    async fn erase(&mut self, block: usize) -> Result<(), StorageError>;
    async fn read_aligned(
        &mut self,
        block: usize,
        offset: usize,
        data: &mut [u8],
    ) -> Result<(), StorageError>;
    async fn write_aligned(
        &mut self,
        block: usize,
        offset: usize,
        data: &[u8],
    ) -> Result<(), StorageError>;
}

impl<T: AlignedStorage> StorageMedium for T
where
    [(); T::PAGE_SIZE]:,
{
    const BLOCK_SIZE: usize = <T as AlignedStorage>::BLOCK_SIZE;
    const BLOCK_COUNT: usize = <T as AlignedStorage>::BLOCK_COUNT;
    const WRITE_GRANULARITY: WriteGranularity = <T as AlignedStorage>::WRITE_GRANULARITY;

    async fn erase(&mut self, block: usize) -> Result<(), StorageError> {
        AlignedStorage::erase(self, block).await
    }

    async fn read(
        &mut self,
        block: usize,
        mut offset: usize,
        mut data: &mut [u8],
    ) -> Result<(), StorageError> {
        if offset % 4 != 0 {
            let align_amt = offset % 4;
            let aligned_offset = offset - align_amt;
            let unaligned_count = (4 - align_amt).min(data.len());

            let mut buffer = AlignedBuffer::from_u32(0);
            self.read_aligned(block, aligned_offset, &mut buffer.data)
                .await?;

            let (unaligned_data, aligned_data) = data.split_at_mut(unaligned_count);
            unaligned_data.copy_from_slice(&buffer.data[align_amt..align_amt + unaligned_count]);
            offset = offset + unaligned_count;
            data = aligned_data;
        }

        if data.len() > 4 {
            // offset is aligned at this point, data may not be
            let shift_after_read = 4 - data.as_ptr() as usize % 4;
            let aligned_buffer = &mut data[shift_after_read..];
            let aligned_len = aligned_buffer.len() - aligned_buffer.len() % 4;
            let mut aligned_buffer = &mut aligned_buffer[0..aligned_len];

            // read aligned data, one (maybe partial) page at a time
            while !aligned_buffer.is_empty() {
                let bytes_in_page = Self::PAGE_SIZE - offset % Self::PAGE_SIZE;
                let bytes_to_read = bytes_in_page.min(aligned_buffer.len());
                self.read_aligned(block, offset, &mut aligned_buffer[..bytes_to_read])
                    .await?;
                aligned_buffer = &mut aligned_buffer[bytes_to_read..];
                offset += bytes_to_read;
            }

            // align data in the slice, if we have to
            if shift_after_read != 0 {
                data.copy_within(shift_after_read..shift_after_read + aligned_len, 0);
            }

            data = &mut data[aligned_len..];
        }

        // read unaligned end of data - in a loop because we might be at a page boundary
        while !data.is_empty() {
            let mut buffer = AlignedBuffer::from_u32(0);
            self.read_aligned(block, offset, &mut buffer.data).await?;

            let copy_len = data.len().min(4);
            let (dst, remaining) = data.split_at_mut(copy_len);
            dst.copy_from_slice(&buffer.data[..copy_len]);
            offset = offset + copy_len;
            data = remaining;
        }

        Ok(())
    }

    async fn write(
        &mut self,
        block: usize,
        mut offset: usize,
        mut data: &[u8],
    ) -> Result<(), StorageError> {
        if offset % 4 != 0 {
            let align_amt = offset % 4;
            let aligned_offset = offset - align_amt;
            let unaligned_count = (4 - align_amt).min(data.len());

            let mut buffer = AlignedBuffer::from_u32(u32::MAX);

            let (unaligned_data, aligned_data) = data.split_at(unaligned_count);
            buffer.data[align_amt..align_amt + unaligned_count].copy_from_slice(unaligned_data);

            self.write_aligned(block, aligned_offset, &buffer.data)
                .await?;

            offset = offset + unaligned_count;
            data = aligned_data;
        };

        if data.as_ptr() as u32 % 4 == 0 {
            while data.len() >= 4 {
                let bytes_in_page = Self::PAGE_SIZE - offset % Self::PAGE_SIZE;
                let bytes_to_write = bytes_in_page.min(data.len());
                let bytes_to_write = bytes_to_write - bytes_to_write % 4;

                self.write_aligned(block, offset, &data[..bytes_to_write])
                    .await?;
                offset += bytes_to_write;
                data = &data[bytes_to_write..];
            }

            if !data.is_empty() {
                let mut buffer = AlignedBuffer::from_u32(u32::MAX);
                buffer.data[..data.len()].copy_from_slice(data);

                self.write_aligned(block, offset, &buffer.data).await?;
            }
        } else {
            let mut buffer = AlignedBuffer::<{ Self::PAGE_SIZE }>::new();

            while !data.is_empty() {
                let bytes_in_page = Self::PAGE_SIZE - offset % Self::PAGE_SIZE;
                let mut bytes_to_write = bytes_in_page.min(data.len());

                let (src, remaining) = data.split_at(bytes_to_write);
                buffer.data[..bytes_to_write].copy_from_slice(&src);

                if bytes_to_write % 4 != 0 {
                    let pad = 4 - bytes_to_write % 4;
                    buffer.data[bytes_to_write..bytes_to_write + pad].fill(0xFF);
                    bytes_to_write += pad;
                }

                self.write_aligned(block, offset, &buffer.data[..bytes_to_write])
                    .await?;
                offset += bytes_to_write;
                data = remaining;
            }
        }

        Ok(())
    }
}
