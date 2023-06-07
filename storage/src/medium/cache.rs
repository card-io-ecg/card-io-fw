use crate::medium::StorageMedium;

use super::WriteGranularity;

pub struct ReadCache<M: StorageMedium, const SIZE: usize> {
    medium: M,
    cache_offset: Option<(usize, usize)>,
    cache: [u8; SIZE],
}

impl<M: StorageMedium, const SIZE: usize> ReadCache<M, SIZE> {
    pub fn new(medium: M) -> Self {
        Self {
            medium,
            cache_offset: None,
            cache: [0; SIZE],
        }
    }

    fn update_cache_if_overlaps(&mut self, block: usize, offset: usize, data: &[u8]) {
        if let Some((cache_block, cache_offset)) = self.cache_offset {
            if cache_block != block {
                return;
            }

            let (cache, data) = if cache_offset > offset {
                (&mut self.cache[..], &data[cache_offset - offset..])
            } else {
                (&mut self.cache[cache_offset..], data)
            };

            cache
                .iter_mut()
                .zip(data)
                .for_each(|(cache, &data)| *cache |= data);
        }
    }

    fn is_cached(&self, block: usize, offset: usize, len: usize) -> bool {
        if let Some((cache_block, cache_offset)) = self.cache_offset {
            if cache_block != block {
                return false;
            }

            offset >= cache_offset && offset + len <= cache_offset + SIZE
        } else {
            false
        }
    }

    async fn load_cache(&mut self, block: usize, offset: usize) -> Result<(), ()> {
        let offset = offset.min(M::BLOCK_SIZE - SIZE);

        self.medium.read(block, offset, &mut self.cache).await?;

        self.cache_offset = Some((block, offset));

        Ok(())
    }

    fn load_from_cache(&self, offset: usize, data: &mut [u8]) {
        let (cache_block, cache_offset) = self.cache_offset.unwrap();
        debug_assert!(self.is_cached(cache_block, offset, data.len()));

        let offset = offset - cache_offset;
        data.copy_from_slice(&self.cache[offset..offset + data.len()]);
    }
}

impl<M: StorageMedium, const SIZE: usize> StorageMedium for ReadCache<M, SIZE> {
    const BLOCK_SIZE: usize = M::BLOCK_SIZE;
    const BLOCK_COUNT: usize = M::BLOCK_COUNT;
    const WRITE_GRANULARITY: WriteGranularity = M::WRITE_GRANULARITY;

    async fn erase(&mut self, block: usize) -> Result<(), ()> {
        if let Some((cache_block, _)) = self.cache_offset {
            if cache_block == block {
                self.cache_offset = None;
            }
        }

        self.medium.erase(block).await
    }

    async fn read(&mut self, block: usize, offset: usize, data: &mut [u8]) -> Result<(), ()> {
        if !self.is_cached(block, offset, data.len()) {
            // We could load partial data from cache,
            // but it's not worth the complexity at this moment.
            if data.len() >= SIZE {
                self.medium.read(block, offset, data).await?;
                return Ok(());
            }

            self.load_cache(block, offset).await?;
        }

        self.load_from_cache(offset, data);

        Ok(())
    }

    async fn write(&mut self, block: usize, offset: usize, data: &[u8]) -> Result<(), ()> {
        let res = self.medium.write(block, offset, data).await?;

        self.update_cache_if_overlaps(block, offset, data);

        Ok(res)
    }
}
