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
            ObjectIterator, ObjectLocation, ObjectOps, ObjectReader, ObjectState, ObjectWriter,
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

enum ObjectKind {
    Header { first_data: u32, next_header: u32 },
    Data { next: u32 },
}

struct Object {
    state: u8,
    kind: ObjectKind,
}

pub struct Reader<'a, P>
where
    P: StorageMedium,
    [(); P::BLOCK_COUNT]:,
{
    storage: &'a mut Storage<P>,
    object: ObjectLocation,
    cursor: u32,
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

    pub async fn read(&mut self, path: &str) -> Result<Reader<'_, P>, ()> {
        let object = self.lookup(path).await?;
        Ok(Reader {
            storage: self,
            object,
            cursor: 0,
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
                        let bytes_read = reader.read(&mut path_buf).await?;
                        let path_bytes = &path.as_bytes()[read..read + bytes_read];

                        if path_bytes != &path_buf[..bytes_read] {
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

    async fn create_new_file(&mut self, path: &str, data: &[u8]) -> Result<(), ()> {
        let path_hash = hash_path(path);

        // Write file name as data object
        let filename_location = self.find_new_object_location(BlockType::Data, data.len())?;

        // filename + 1 data page
        let est_page_count = 1 + 1; // TODO: guess the number of data pages needed

        let file_meta_location = self.find_new_object_location(
            BlockType::Metadata,
            est_page_count * P::object_location_bytes(),
        )?;

        let mut meta_writer = ObjectWriter::new(file_meta_location, &mut self.medium).await?;
        let mut filename_writer = ObjectWriter::new(filename_location, &mut self.medium).await?;

        filename_writer.allocate(&mut self.medium).await?;
        filename_writer
            .write(&mut self.medium, path.as_bytes())
            .await?;
        filename_writer.finalize(&mut self.medium).await?;

        // Write a non-finalized header obejct
        let (fn_loc_bytes, fn_loc_byte_count) = filename_location.into_bytes::<P>();

        meta_writer.allocate(&mut self.medium).await?;
        meta_writer
            .write(&mut self.medium, &path_hash.to_le_bytes())
            .await?;
        meta_writer
            .write(&mut self.medium, &fn_loc_bytes[..fn_loc_byte_count])
            .await?;

        // Write data objects
        // TODO

        // Finalize header object
        meta_writer.finalize(&mut self.medium).await?;

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
}
