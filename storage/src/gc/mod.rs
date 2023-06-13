use crate::{
    ll::{
        blocks::{BlockHeaderKind, BlockInfo, BlockOps},
        objects::{ObjectIterator, ObjectState},
    },
    medium::{StorageMedium, StoragePrivate},
    StorageError,
};

pub struct Gc<'a, M: StorageMedium> {
    medium: &'a mut M,
    block_info: &'a mut [BlockInfo<M>],
}

impl<'a, M: StorageMedium> Gc<'a, M> {
    pub fn new(medium: &'a mut M, block_info: &'a mut [BlockInfo<M>]) -> Self {
        Self { medium, block_info }
    }

    async fn delete_invalid_objects_in_block(
        medium: &mut M,
        block: usize,
        info: &mut BlockInfo<M>,
    ) -> Result<(), StorageError> {
        if info.header.kind().is_unknown() {
            return Err(StorageError::FsCorrupted);
        }
        if info.header.kind().is_empty() {
            return Ok(());
        }

        let mut iter = ObjectIterator::new::<M>(block);

        while let Some(mut object) = iter.next(medium).await? {
            if object.header.state == ObjectState::Allocated {
                let mut delete = true;

                if object.header.object_size == u32::MAX as usize {
                    object
                        .header
                        .set_payload_size(medium, info.used_bytes)
                        .await?;
                } else {
                    // We can clean up objects that seem to have a valid size. However, we can't
                    // clean up objects where the size doesn't match up because that would leave the
                    // block in an invalid state.
                    delete = object.header.object_size
                        != info.used_bytes - object.location.offset - M::object_header_bytes();
                }

                if !delete {
                    return Ok(());
                }

                object
                    .header
                    .update_state(medium, ObjectState::Deleted)
                    .await?;

                info.allow_alloc = true;

                // Continuing to loop here should just implicitly exit.
            }
        }

        Ok(())
    }

    /// This function deletes all objects that are in an invalid state, which can be caused by a
    /// power loss during a write operation.
    async fn delete_invalid_objects(&mut self) -> Result<(), StorageError> {
        log::trace!("GC::delete_invalid_objects()");
        for (block, info) in self.block_info.iter_mut().enumerate() {
            Self::delete_invalid_objects_in_block(self.medium, block, info).await?;
        }

        Ok(())
    }

    async fn erase_invalid_finished_block(
        medium: &mut M,
        block: usize,
        info: &mut BlockInfo<M>,
    ) -> Result<(), StorageError> {
        if info.allow_alloc {
            return Ok(());
        }

        let mut iter = ObjectIterator::new::<M>(block);

        let mut erase = false;
        while let Some(object) = iter.next(medium).await? {
            if object.header.state != ObjectState::Deleted {
                erase = object.header.state == ObjectState::Allocated;
                break;
            }
        }

        if !erase {
            return Ok(());
        }

        if iter.next(medium).await?.is_some() {
            // We should not have blocks where an Allocated object is not the last one.
            return Err(StorageError::FsCorrupted);
        }

        let BlockHeaderKind::Known(_) = info.header.kind() else {
            // We should have fixed the invalid blocks first
            return Err(StorageError::FsCorrupted);
        };

        BlockOps::new(medium).format_block(block).await?;

        info.update_stats_after_erase();

        Ok(())
    }

    /// This function erases blocks where new objects can't be allocated anymore and all present
    /// objects are deleted.
    ///
    /// This function should be called after `delete_invalid_objects` to give the GC a chance to
    /// fix an invalid block.
    async fn erase_invalid_finished_blocks(&mut self) -> Result<(), StorageError> {
        log::trace!("GC::erase_invalid_finished_blocks()");
        for (block, info) in self.block_info.iter_mut().enumerate() {
            Self::erase_invalid_finished_block(self.medium, block, info).await?;
        }

        Ok(())
    }

    async fn erase_full_finished_block(
        medium: &mut M,
        block: usize,
        info: &mut BlockInfo<M>,
    ) -> Result<(), StorageError> {
        if info.used_bytes >= M::BLOCK_SIZE - M::object_header_bytes() {
            return Ok(());
        }

        let mut iter = ObjectIterator::new::<M>(block);

        while let Some(object) = iter.next(medium).await? {
            if object.header.state != ObjectState::Deleted {
                return Ok(());
            }
        }

        let BlockHeaderKind::Known(_) = info.header.kind() else {
            // We should have fixed the invalid blocks first
            return Err(StorageError::FsCorrupted);
        };

        BlockOps::new(medium).format_block(block).await?;

        info.update_stats_after_erase();

        Ok(())
    }

    /// This function erases blocks where all present objects are deleted and the block is full.
    async fn erase_full_finished_blocks(&mut self) -> Result<(), StorageError> {
        log::trace!("GC::erase_full_finished_blocks()");
        for (block, info) in self.block_info.iter_mut().enumerate() {
            Self::erase_full_finished_block(self.medium, block, info).await?;
        }

        Ok(())
    }

    pub async fn run(&mut self) -> Result<(), StorageError> {
        self.delete_invalid_objects().await?;
        self.erase_invalid_finished_blocks().await?;
        self.erase_full_finished_blocks().await?;

        Ok(())
    }
}
