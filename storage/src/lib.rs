#![cfg_attr(not(test), no_std)]
#![feature(async_fn_in_trait)]
#![feature(impl_trait_projections)]
#![feature(generic_const_exprs)] // Eww
#![allow(incomplete_features)]

use crate::{
    ll::{
        blocks::{BlockHeaderKind, BlockInfo, BlockOps, BlockType},
        objects::{ObjectIterator, ObjectState},
    },
    medium::StorageMedium,
};

pub mod gc;
pub mod ll;
pub mod medium;

pub struct Storage<P>
where
    P: StorageMedium,
    [(); P::BLOCK_COUNT]:,
{
    media: P,
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
    object: ObjectId,
    cursor: u32,
}

struct ObjectId {
    offset: u32,
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
            media: partition,
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
        todo!()
    }

    async fn lookup(&mut self, path: &str) -> Result<ObjectId, ()> {
        let path_hash = path.len(); // TODO: Hash the path

        for block_idx in self
            .blocks
            .iter()
            .enumerate()
            .filter_map(|(idx, blk)| blk.is_metadata().then_some(idx))
        {
            let mut iter = ObjectIterator::new(block_idx);

            while let Some(object) = iter.next(&mut self.media).await? {
                if object.header.state != ObjectState::Finalized {
                    continue;
                }

                let object_hash = 0; // TODO

                if object_hash == path_hash {
                    todo!("Read first data object and compare path. If path matches, return object id.");
                }
            }
        }

        // not found
        Err(())
    }

    async fn delete_object(&mut self, object: ObjectId) -> Result<(), ()> {
        todo!()
    }

    async fn allocate_object(&mut self, path: &str) -> Result<ObjectId, ()> {
        todo!()
    }

    async fn write_object(&mut self, object: &ObjectId, data: &[u8]) -> Result<(), ()> {
        todo!()
    }
}
