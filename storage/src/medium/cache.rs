use crate::{medium::StorageMedium, StorageError};

use super::WriteGranularity;

struct CachePage<const SIZE: usize> {
    location: Option<(usize, usize)>,
    cache: [u8; SIZE],
}

impl<const SIZE: usize> CachePage<SIZE> {
    const EMPTY: Self = Self::new();

    const fn new() -> Self {
        Self {
            location: None,
            cache: [0; SIZE],
        }
    }

    fn update_overlapping(&mut self, block: usize, offset: usize, data: &[u8]) {
        let Some((cache_block, cache_offset)) = self.location else {
            return;
        };

        if cache_block != block {
            return;
        }

        if offset >= cache_offset + SIZE || offset + data.len() <= cache_offset {
            return;
        }

        let (cache_range, data_range) = if cache_offset > offset {
            // We don't store the first part of the data
            (0.., cache_offset - offset..)
        } else {
            // Data starts inside the cache
            (offset - cache_offset.., 0..)
        };

        self.cache[cache_range]
            .iter_mut()
            .zip(&data[data_range])
            .for_each(|(cache, &data)| *cache &= data);
    }

    fn is_in_cache(&self, block: usize, offset: usize, len: usize) -> bool {
        let Some((cache_block, cache_offset)) = self.location else {
            return false;
        };

        if cache_block != block {
            return false;
        }

        offset >= cache_offset && offset + len <= cache_offset + SIZE
    }

    fn erase_block(&mut self, block: usize) {
        let Some((cache_block, _cache_offset)) = self.location else {
            return;
        };

        if cache_block != block {
            return;
        }

        self.location = None;
    }

    fn load_from_cache(&self, block: usize, offset: usize, data: &mut [u8]) {
        let (_cache_block, cache_offset) = self.location.unwrap();
        debug_assert!(self.is_in_cache(block, offset, data.len()));

        let offset = offset - cache_offset;
        data.copy_from_slice(&self.cache[offset..offset + data.len()]);
    }
}

struct CachePages<const SIZE: usize, const COUNT: usize> {
    pages: [CachePage<SIZE>; COUNT],
    used: [usize; COUNT],
    used_count: usize,
}

impl<const SIZE: usize, const COUNT: usize> CachePages<SIZE, COUNT> {
    const COUNT_AT_LEAST_1: () = assert!(COUNT > 0);

    fn new() -> Self {
        let _ = Self::COUNT_AT_LEAST_1;

        Self {
            pages: [CachePage::EMPTY; COUNT],
            used: [0; COUNT],
            used_count: 0,
        }
    }

    fn update_overlapping(&mut self, block: usize, offset: usize, data: &[u8]) {
        for page in self.pages.iter_mut() {
            page.update_overlapping(block, offset, data);
        }
    }

    fn get_if_in_cache(
        &mut self,
        block: usize,
        offset: usize,
        len: usize,
    ) -> Option<&CachePage<SIZE>> {
        fn mark_used<const COUNT: usize>(idx: usize, used: &mut [usize; COUNT]) {
            let position = used.iter().position(|&x| x == idx).unwrap();

            used.copy_within(position + 1.., position);
            used[COUNT - 1] = idx;
        }

        for (idx, page) in self.pages.iter().enumerate() {
            if page.is_in_cache(block, offset, len) {
                mark_used(idx, &mut self.used);
                return Some(page);
            }
        }

        None
    }

    fn erase_pages_in_block(&mut self, block: usize) {
        for page in self.pages.iter_mut() {
            page.erase_block(block);
        }
    }

    fn allocate_cache_page(&mut self) -> &mut CachePage<SIZE> {
        let idx_to_allocate;
        if self.used_count != COUNT {
            idx_to_allocate = self.used_count;
            self.used_count += 1;
        } else {
            idx_to_allocate = self.used[0];
        }

        self.used.copy_within(1.., 0);
        self.used[COUNT - 1] = idx_to_allocate;

        self.pages[idx_to_allocate].location = None;

        &mut self.pages[idx_to_allocate]
    }
}

pub struct ReadCache<M: StorageMedium, const SIZE: usize, const PAGES: usize> {
    medium: M,
    cache: CachePages<SIZE, PAGES>,
}

impl<M: StorageMedium, const SIZE: usize, const PAGES: usize> ReadCache<M, SIZE, PAGES> {
    pub fn new(medium: M) -> Self {
        Self {
            medium,
            cache: CachePages::new(),
        }
    }

    fn update_cache_if_overlaps(&mut self, block: usize, offset: usize, data: &[u8]) {
        self.cache.update_overlapping(block, offset, data);
    }

    fn is_cached(&mut self, block: usize, offset: usize, len: usize) -> bool {
        self.cache.get_if_in_cache(block, offset, len).is_some()
    }

    async fn load_into_cache(&mut self, block: usize, offset: usize) -> Result<(), StorageError> {
        let page = self.cache.allocate_cache_page();
        let offset = offset.min(M::BLOCK_SIZE - SIZE);

        self.medium.read(block, offset, &mut page.cache).await?;

        page.location = Some((block, offset));

        Ok(())
    }

    fn load_from_cache(&mut self, block: usize, offset: usize, data: &mut [u8]) {
        let page = self
            .cache
            .get_if_in_cache(block, offset, data.len())
            .unwrap();

        page.load_from_cache(block, offset, data);
    }
}

impl<M: StorageMedium, const SIZE: usize, const PAGES: usize> StorageMedium
    for ReadCache<M, SIZE, PAGES>
{
    const BLOCK_SIZE: usize = M::BLOCK_SIZE;
    const BLOCK_COUNT: usize = M::BLOCK_COUNT;
    const WRITE_GRANULARITY: WriteGranularity = M::WRITE_GRANULARITY;

    async fn erase(&mut self, block: usize) -> Result<(), StorageError> {
        self.cache.erase_pages_in_block(block);

        self.medium.erase(block).await
    }

    async fn read(
        &mut self,
        block: usize,
        offset: usize,
        data: &mut [u8],
    ) -> Result<(), StorageError> {
        if !self.is_cached(block, offset, data.len()) {
            // We could load partial data from cache,
            // but it's not worth the complexity at this moment.
            if data.len() >= SIZE {
                self.medium.read(block, offset, data).await?;
                return Ok(());
            }

            self.load_into_cache(block, offset).await?;
        }

        self.load_from_cache(block, offset, data);

        Ok(())
    }

    async fn write(
        &mut self,
        block: usize,
        offset: usize,
        data: &[u8],
    ) -> Result<(), StorageError> {
        self.medium.write(block, offset, data).await?;

        self.update_cache_if_overlaps(block, offset, data);

        Ok(())
    }
}
