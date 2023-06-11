#![cfg_attr(not(test), no_std)]
#![feature(async_fn_in_trait)]
#![feature(impl_trait_projections)]
#![feature(generic_const_exprs)] // Eww
#![allow(incomplete_features)]

use crate::{
    ll::blocks::{BlockInfo, BlockOps},
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
    partition: P,
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

        Ok(Self { partition, blocks })
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

    pub fn delete(&mut self, path: &str) -> Result<(), ()> {
        let object = self.lookup(path)?;
        self.delete_object(object)
    }

    pub fn store(&mut self, path: &str, data: &[u8]) -> Result<(), ()> {
        let object = self.lookup(path);

        let new_object = self.allocate_object(path)?;
        self.write_object(&new_object, data)?;
        self.finalize(new_object)?;

        if let Ok(object) = object {
            self.delete_object(object)?;
        }

        Ok(())
    }

    pub fn read(&mut self, path: &str) -> Result<Reader<'_, P>, ()> {
        let object = self.lookup(path)?;
        todo!()
    }

    fn lookup(&mut self, path: &str) -> Result<ObjectId, ()> {
        todo!()
    }

    fn delete_object(&mut self, object: ObjectId) -> Result<(), ()> {
        todo!()
    }

    fn allocate_object(&mut self, path: &str) -> Result<ObjectId, ()> {
        todo!()
    }

    fn write_object(&mut self, object: &ObjectId, data: &[u8]) -> Result<(), ()> {
        todo!()
    }

    fn finalize(&mut self, object: ObjectId) -> Result<(), ()> {
        todo!()
    }
}
