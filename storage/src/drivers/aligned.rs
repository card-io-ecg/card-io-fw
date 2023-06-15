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
            let buffer_range = align_amt..align_amt + unaligned_count;

            let mut buffer = AlignedBuffer::from_u32(0);
            self.read_aligned(block, aligned_offset, &mut buffer.data)
                .await?;

            let (dst, remaining) = data.split_at_mut(unaligned_count);
            dst.copy_from_slice(&buffer.data[buffer_range]);

            offset += dst.len();
            data = remaining;
        }

        if data.len() > 4 {
            // offset is aligned at this point, data may not be
            let shift_after_read = (4 - data.as_ptr() as usize % 4) % 4;
            let aligned_buffer = &mut data[shift_after_read..];
            let aligned_len = aligned_buffer.len() - aligned_buffer.len() % 4;
            let mut aligned_buffer = &mut aligned_buffer[..aligned_len];

            // read aligned data, one (maybe partial) page at a time
            while !aligned_buffer.is_empty() {
                let bytes_in_page = Self::PAGE_SIZE - offset % Self::PAGE_SIZE;
                let bytes_to_read = bytes_in_page.min(aligned_buffer.len());

                let (dst, remaining) = aligned_buffer.split_at_mut(bytes_to_read);
                self.read_aligned(block, offset, dst).await?;

                offset += dst.len();
                aligned_buffer = remaining;
            }

            // align data in the slice, if we have to
            if shift_after_read != 0 {
                debug_assert!(shift_after_read < 4);
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

            offset += dst.len();
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
            let buffer_range = align_amt..align_amt + unaligned_count;

            let mut buffer = AlignedBuffer::from_u32(u32::MAX);

            let (unaligned_data, remaining) = data.split_at(unaligned_count);
            buffer.data[buffer_range].copy_from_slice(unaligned_data);

            self.write_aligned(block, aligned_offset, &buffer.data)
                .await?;

            offset += unaligned_count;
            data = remaining;
        };

        if data.as_ptr() as u32 % 4 == 0 {
            while data.len() > 4 {
                let bytes_in_page = Self::PAGE_SIZE - offset % Self::PAGE_SIZE;
                let bytes_to_write = bytes_in_page.min(data.len());

                let bytes_to_write = bytes_to_write - bytes_to_write % 4;
                let (src, remaining) = data.split_at(bytes_to_write);

                self.write_aligned(block, offset, src).await?;
                offset += src.len();
                data = remaining;
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

                let src = &buffer.data[..bytes_to_write];

                self.write_aligned(block, offset, src).await?;
                offset += src.len();
                data = remaining;
            }
        }

        Ok(())
    }
}
