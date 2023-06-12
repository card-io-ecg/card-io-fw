#![cfg_attr(not(test), no_std)]
#![feature(async_fn_in_trait)]
#![feature(impl_trait_projections)]
#![feature(generic_const_exprs)] // Eww
#![allow(incomplete_features)]

use crate::{
    diag::Counters,
    ll::{
        blocks::{BlockHeaderKind, BlockInfo, BlockOps, BlockType},
        objects::{
            MetadataObjectHeader, ObjectIterator, ObjectLocation, ObjectOps, ObjectReader,
            ObjectState, ObjectWriter,
        },
    },
    medium::{StorageMedium, StoragePrivate},
};

pub mod diag;
pub mod gc;
pub mod ll;
pub mod medium;

pub struct Storage<P>
where
    P: StorageMedium,
    [(); P::BLOCK_COUNT]:,
{
    medium: P,
    blocks: [BlockInfo<P>; P::BLOCK_COUNT],
}

pub struct Reader<P>
where
    P: StorageMedium,
    [(); P::BLOCK_COUNT]:,
{
    meta: MetadataObjectHeader<P>,
    current_object: Option<ObjectReader<P>>,
}

impl<P> Reader<P>
where
    P: StorageMedium,
    [(); P::BLOCK_COUNT]:,
{
    pub async fn read(&mut self, storage: &mut Storage<P>, buf: &mut [u8]) -> Result<usize, ()> {
        let medium = &mut storage.medium;

        if self.current_object.is_none() {
            if let Some(object) = self.meta.next_object_location(medium).await? {
                self.current_object = Some(ObjectReader::new(object, medium).await?);
            } else {
                return Ok(0);
            }
        }

        let current_object = self.current_object.as_mut().unwrap();
        current_object.read(&mut storage.medium, buf).await
    }
}

impl<P> Storage<P>
where
    P: StorageMedium,
    [(); P::BLOCK_COUNT]:,
{
    pub async fn mount(mut partition: P) -> Result<Self, ()> {
        let mut blocks = [BlockInfo::new_unknown(); P::BLOCK_COUNT];

        let mut ops = BlockOps::new(&mut partition);
        for block in 0..P::BLOCK_COUNT {
            blocks[block] = ops.scan_block(block).await?;
        }

        Ok(Self {
            medium: partition,
            blocks,
        })
    }

    pub async fn format(partition: &mut P, metadata_blocks: usize) -> Result<(), ()> {
        BlockOps::new(partition)
            .format_storage(metadata_blocks)
            .await
    }

    pub async fn format_and_mount(mut partition: P, metadata_blocks: usize) -> Result<Self, ()> {
        Self::format(&mut partition, metadata_blocks).await?;

        Self::mount(partition).await
    }

    pub async fn delete(&mut self, path: &str) -> Result<(), ()> {
        let location = self.lookup(path).await?;
        self.delete_file_at(location).await
    }

    pub async fn store(&mut self, path: &str, data: &[u8]) -> Result<(), ()> {
        let overwritten_location = self.lookup(path).await;

        self.create_new_file(path, data).await?;

        if let Ok(location) = overwritten_location {
            self.delete_file_at(location).await?;
        }

        Ok(())
    }

    pub async fn read(&mut self, path: &str) -> Result<Reader<P>, ()> {
        let object = self.lookup(path).await?;
        Ok(Reader {
            meta: object.read_metadata(&mut self.medium).await?,
            current_object: None,
        })
    }

    async fn lookup(&mut self, path: &str) -> Result<ObjectLocation, ()> {
        let path_hash = hash_path(path);

        for block_idx in self
            .blocks
            .iter()
            .enumerate()
            .filter_map(|(idx, blk)| blk.is_metadata().then_some(idx))
        {
            let mut iter = ObjectIterator::new(block_idx);

            'objs: while let Some(object) = iter.next(&mut self.medium).await? {
                if object.header.state != ObjectState::Finalized {
                    continue 'objs;
                }

                let metadata = object.read_metadata(&mut self.medium).await?;

                if metadata.path_hash == path_hash {
                    let mut reader =
                        ObjectReader::new(metadata.filename_location, &mut self.medium).await?;

                    if reader.len() != path.len() {
                        continue 'objs;
                    }

                    let mut path_buf = [0u8; 16];

                    let mut read = 0;
                    while read < path.len() {
                        let len = path_buf.len().min(path.len() - read);
                        let buf = &mut path_buf[..len];

                        let bytes_read = reader.read(&mut self.medium, buf).await?;
                        let path_bytes = &path.as_bytes()[read..read + bytes_read];

                        if path_bytes != buf {
                            continue 'objs;
                        }

                        read += bytes_read;
                    }

                    return Ok(metadata.location);
                }
            }
        }

        // not found
        Err(())
    }

    async fn delete_file_at(&mut self, meta_location: ObjectLocation) -> Result<(), ()> {
        let mut metadata = meta_location.read_metadata(&mut self.medium).await?;
        let mut ops = ObjectOps::new(&mut self.medium);

        ops.update_state(metadata.filename_location, ObjectState::Deleted)
            .await?;

        while let Some(location) = metadata.next_object_location(ops.medium).await? {
            ops.update_state(location, ObjectState::Deleted).await?;
        }

        ops.update_state(meta_location, ObjectState::Deleted)
            .await?;

        Ok(())
    }

    async fn write_object(&mut self, location: ObjectLocation, data: &[u8]) -> Result<(), ()> {
        self.blocks[location.block].used_bytes +=
            ObjectWriter::write_to(location, &mut self.medium, data).await?;
        Ok(())
    }

    async fn write_location(
        &mut self,
        meta_writer: &mut ObjectWriter<P>,
        location: ObjectLocation,
    ) -> Result<(), ()> {
        let (bytes, byte_count) = location.into_bytes::<P>();
        meta_writer
            .write(&mut self.medium, &bytes[..byte_count])
            .await
    }

    async fn create_new_file(&mut self, path: &str, mut data: &[u8]) -> Result<(), ()> {
        let path_hash = hash_path(path);

        // Write file name as data object
        let filename_location = self.find_new_object_location(BlockType::Data, path.len())?;

        // filename + 1 data page
        let est_page_count = 1 + 1; // TODO: guess the number of data pages needed

        let file_meta_location = self.find_new_object_location(
            BlockType::Metadata,
            est_page_count * P::object_location_bytes(),
        )?;

        let mut meta_writer = ObjectWriter::new(file_meta_location, &mut self.medium).await?;

        self.write_object(filename_location, path.as_bytes())
            .await?;

        // Write a non-finalized header obejct
        meta_writer.allocate(&mut self.medium).await?;
        meta_writer
            .write(&mut self.medium, &path_hash.to_le_bytes())
            .await?;

        self.write_location(&mut meta_writer, filename_location)
            .await?;

        // Write data objects
        while !data.is_empty() {
            // Write file name as data object
            let chunk_location = self.find_new_object_location(BlockType::Data, 0)?;
            let max_chunk_len =
                self.blocks[chunk_location.block].free_space() - P::object_header_bytes();

            let (chunk, remaining) = data.split_at(data.len().min(max_chunk_len));
            data = remaining;

            self.write_object(chunk_location, chunk).await?;

            self.write_location(&mut meta_writer, chunk_location)
                .await?;
        }

        // TODO: store data length
        // Finalize header object
        let object_total_size = meta_writer.finalize(&mut self.medium).await?;
        self.blocks[file_meta_location.block].used_bytes += object_total_size;

        Ok(())
    }

    fn find_alloc_block(&self, ty: BlockType, min_free: usize) -> Result<usize, ()> {
        // Try to find a used block with enough free space
        if let Some(block) = self.blocks.iter().position(|info| {
            info.header.kind() == BlockHeaderKind::Known(ty)
                && !info.is_empty()
                && info.free_space() >= min_free
        }) {
            return Ok(block);
        }

        // Pick a free block
        if let Some(block) = self.blocks.iter().position(|info| {
            info.header.kind() == BlockHeaderKind::Known(ty) && info.free_space() >= min_free
        }) {
            return Ok(block);
        }

        // No block found
        Err(())
    }

    fn find_new_object_location(&self, ty: BlockType, len: usize) -> Result<ObjectLocation, ()> {
        // find block with most free space
        let block = self.find_alloc_block(ty, P::object_header_bytes() + len)?;

        Ok(ObjectLocation {
            block,
            offset: self.blocks[block].used_bytes,
        })
    }
}

fn hash_path(path: &str) -> u32 {
    // TODO
    path.len() as u32
}

impl<P> Storage<Counters<P>>
where
    P: StorageMedium,
    [(); P::BLOCK_COUNT]:,
    [(); Counters::<P>::BLOCK_COUNT]:,
{
    pub fn erase_count(&self) -> usize {
        self.medium.erase_count
    }

    pub fn read_count(&self) -> usize {
        self.medium.read_count
    }

    pub fn write_count(&self) -> usize {
        self.medium.write_count
    }
}

#[cfg(test)]
mod test {
    use crate::medium::ram::RamStorage;

    use super::*;

    async fn create_fs() -> Storage<RamStorage<256, 32>> {
        let medium = RamStorage::<256, 32>::new();
        Storage::format_and_mount(medium, 3)
            .await
            .expect("Failed to mount storage")
    }

    #[async_std::test]
    async fn lookup_returns_error_if_file_does_not_exist() {
        let mut storage = create_fs().await;

        assert!(
            storage.read("foo").await.is_err(),
            "Lookup returned Ok unexpectedly"
        );
    }

    #[async_std::test]
    async fn delete_returns_error_if_file_does_not_exist() {
        let mut storage = create_fs().await;

        storage
            .delete("foo")
            .await
            .expect_err("Delete returned Ok unexpectedly");
    }

    #[async_std::test]
    async fn written_file_can_be_read() {
        let mut storage = create_fs().await;

        storage
            .create_new_file("foo", b"barbaz")
            .await
            .expect("Create failed");

        storage.medium.debug_print();

        let mut reader = storage.read("foo").await.expect("Failed to open file");

        let mut buf = [0u8; 6];

        reader
            .read(&mut storage, &mut buf)
            .await
            .expect("Failed to read file");

        assert_eq!(buf, *b"barbaz");
    }
}
