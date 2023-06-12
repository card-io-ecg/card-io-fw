#![cfg_attr(not(test), no_std)]
#![feature(async_fn_in_trait)]
#![feature(impl_trait_projections)]
#![feature(generic_const_exprs)] // Eww
#![allow(incomplete_features)]

use crate::{
    diag::Counters,
    ll::{
        blocks::{BlockInfo, BlockOps},
        objects::{ObjectIterator, ObjectLocation, ObjectReader, ObjectState},
    },
    medium::StorageMedium,
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
        let object = self.lookup(path).await?;
        self.delete_object(object).await
    }

    pub async fn store(&mut self, path: &str, data: &[u8]) -> Result<(), ()> {
        let object = self.lookup(path).await;

        let new_object = self.allocate_object(path).await?;
        self.write_object(&new_object, data).await?;

        if let Ok(object) = object {
            self.delete_object(object).await?;
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
        let path_hash = path.len() as u32; // TODO: Hash the path

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

    async fn delete_object(&mut self, object: ObjectLocation) -> Result<(), ()> {
        todo!()
    }

    async fn allocate_object(&mut self, path: &str) -> Result<ObjectLocation, ()> {
        todo!()
    }

    async fn write_object(&mut self, object: &ObjectLocation, data: &[u8]) -> Result<(), ()> {
        todo!()
    }
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
