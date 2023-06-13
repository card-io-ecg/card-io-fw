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
            MetadataObjectHeader, ObjectHeader, ObjectIterator, ObjectLocation, ObjectReader,
            ObjectState, ObjectWriter,
        },
    },
    medium::{StorageMedium, StoragePrivate},
};

pub mod diag;
pub mod fxhash;
pub mod gc;
pub mod ll;
pub mod medium;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StorageError {
    NotFound,
    FsCorrupted,
    Io,
    InvalidOperation,
    InsufficientSpace,
}

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
    async fn select_next_object(&mut self, medium: &mut P) -> Result<(), StorageError> {
        self.current_object = if let Some(object) = self.meta.next_object_location(medium).await? {
            Some(ObjectReader::new(object, medium, false).await?)
        } else {
            None
        };

        Ok(())
    }

    pub async fn read(
        &mut self,
        storage: &mut Storage<P>,
        mut buf: &mut [u8],
    ) -> Result<usize, StorageError> {
        let medium = &mut storage.medium;

        if self.current_object.is_none() {
            self.select_next_object(medium).await?;
        }

        let len = buf.len();

        loop {
            let Some(reader) = self.current_object.as_mut() else {
                // EOF
                break;
            };

            let read = reader.read(medium, buf).await?;
            buf = &mut buf[read..];

            if buf.is_empty() {
                // Buffer is full
                break;
            }

            self.select_next_object(medium).await?;
        }

        Ok(len - buf.len())
    }
}

impl<P> Storage<P>
where
    P: StorageMedium,
    [(); P::BLOCK_COUNT]:,
{
    pub async fn mount(mut partition: P) -> Result<Self, StorageError> {
        let mut blocks = [BlockInfo::new_unknown(); P::BLOCK_COUNT];

        let mut ops = BlockOps::new(&mut partition);
        for (idx, block) in blocks.iter_mut().enumerate() {
            *block = ops.scan_block(idx).await?;
        }

        Ok(Self {
            medium: partition,
            blocks,
        })
    }

    pub async fn format(partition: &mut P) -> Result<(), StorageError> {
        BlockOps::new(partition).format_storage().await
    }

    pub async fn format_and_mount(mut partition: P) -> Result<Self, StorageError> {
        Self::format(&mut partition).await?;

        Self::mount(partition).await
    }

    pub async fn delete(&mut self, path: &str) -> Result<(), StorageError> {
        log::trace!("Storage::delete({path})");
        let location = self.lookup(path).await?;
        self.delete_file_at(location).await
    }

    pub async fn store(&mut self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        log::trace!("Storage::store({path}, len = {})", data.len());
        let overwritten_location = self.lookup(path).await;

        self.create_new_file(path, data).await?;

        if let Ok(location) = overwritten_location {
            self.delete_file_at(location).await?;
        }

        Ok(())
    }

    /// Convenience method for checking if a file exists. Ignores all errors.
    pub async fn exists(&mut self, path: &str) -> bool {
        log::trace!("Storage::exists({path})");
        self.lookup(path).await.is_ok()
    }

    pub async fn read(&mut self, path: &str) -> Result<Reader<P>, StorageError> {
        log::trace!("Storage::read({path})");
        let object = self.lookup(path).await?;
        Ok(Reader {
            meta: object.read_metadata(&mut self.medium).await?,
            current_object: None,
        })
    }

    async fn lookup(&mut self, path: &str) -> Result<ObjectLocation, StorageError> {
        let path_hash = hash_path(path);

        for block_idx in self
            .blocks
            .iter()
            .enumerate()
            .filter_map(|(idx, blk)| blk.is_metadata().then_some(idx))
        {
            let mut iter = ObjectIterator::new::<P>(block_idx);

            'objs: while let Some(object) = iter.next(&mut self.medium).await? {
                if object.header.state != ObjectState::Finalized {
                    continue 'objs;
                }

                let metadata = object.read_metadata(&mut self.medium).await?;

                if metadata.path_hash == path_hash {
                    let mut reader =
                        ObjectReader::new(metadata.filename_location, &mut self.medium, false)
                            .await?;

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
        Err(StorageError::NotFound)
    }

    async fn delete_file_at(&mut self, meta_location: ObjectLocation) -> Result<(), StorageError> {
        let mut metadata = meta_location.read_metadata(&mut self.medium).await?;

        metadata
            .object
            .update_state(&mut self.medium, ObjectState::Deleted)
            .await?;

        while let Some(location) = metadata.next_object_location(&mut self.medium).await? {
            let mut header = ObjectHeader::read(location, &mut self.medium).await?;
            header
                .update_state(&mut self.medium, ObjectState::Deleted)
                .await?;
        }

        metadata
            .object
            .update_state(&mut self.medium, ObjectState::Deleted)
            .await?;

        Ok(())
    }

    async fn write_object(
        &mut self,
        location: ObjectLocation,
        data: &[u8],
    ) -> Result<(), StorageError> {
        self.blocks[location.block].used_bytes +=
            ObjectWriter::write_to(location, &mut self.medium, data).await?;
        Ok(())
    }

    async fn write_location(
        &mut self,
        meta_writer: &mut ObjectWriter<P>,
        location: ObjectLocation,
    ) -> Result<(), StorageError> {
        let (bytes, byte_count) = location.into_bytes::<P>();
        meta_writer
            .write(&mut self.medium, &bytes[..byte_count])
            .await
    }

    async fn create_new_file(&mut self, path: &str, mut data: &[u8]) -> Result<(), StorageError> {
        let path_hash = hash_path(path);

        // filename + 1 data page
        let est_page_count = 1 + 1; // TODO: guess the number of data pages needed

        // this is mutable because we can fail mid-writing
        let mut file_meta_location = self
            .find_new_object_location(
                BlockType::Metadata,
                est_page_count * P::object_location_bytes(),
            )
            .await?;

        // Write file name as data object
        let filename_location = self
            .find_new_object_location(BlockType::Data, path.len())
            .await?;

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
            let chunk_location = self.find_new_object_location(BlockType::Data, 0).await?;
            let max_chunk_len =
                self.blocks[chunk_location.block].free_space() - P::object_header_bytes();

            let (chunk, remaining) = data.split_at(data.len().min(max_chunk_len));
            data = remaining;

            self.write_object(chunk_location, chunk).await?;

            match self.write_location(&mut meta_writer, chunk_location).await {
                Ok(()) => {}
                Err(StorageError::InsufficientSpace) => {
                    // Old object's accounting
                    self.blocks[file_meta_location.block].used_bytes += meta_writer.total_size();

                    let new_file_meta_location = self
                        .find_new_object_location(
                            BlockType::Metadata,
                            meta_writer.payload_size() + P::object_location_bytes(),
                        )
                        .await?;

                    let mut new_meta_writer =
                        ObjectWriter::new(new_file_meta_location, &mut self.medium).await?;

                    new_meta_writer.allocate(&mut self.medium).await?;

                    // TODO: seek over object size when added - it should be the first for simplicity

                    // Copy old object
                    let mut buf = [0u8; 16];
                    let mut old_object_reader =
                        ObjectReader::new(file_meta_location, &mut self.medium, false).await?;
                    loop {
                        let bytes_read = old_object_reader.read(&mut self.medium, &mut buf).await?;

                        if bytes_read == 0 {
                            break;
                        }

                        new_meta_writer
                            .write(&mut self.medium, &buf[..bytes_read])
                            .await?;
                    }

                    meta_writer.delete(&mut self.medium).await?;

                    meta_writer = new_meta_writer;
                    file_meta_location = new_file_meta_location;
                }
                Err(e) => return Err(e),
            }
        }

        // TODO: store data length
        // Finalize header object
        let object_total_size = meta_writer.finalize(&mut self.medium).await?;
        self.blocks[file_meta_location.block].used_bytes += object_total_size;

        Ok(())
    }

    fn find_alloc_block(&self, ty: BlockType, min_free: usize) -> Result<usize, StorageError> {
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
            info.header.kind() == BlockHeaderKind::Known(BlockType::Undefined)
                && info.free_space() >= min_free
        }) {
            return Ok(block);
        }

        // No block found
        Err(StorageError::InsufficientSpace)
    }

    async fn find_new_object_location(
        &mut self,
        ty: BlockType,
        len: usize,
    ) -> Result<ObjectLocation, StorageError> {
        // find block with most free space
        let block = self.find_alloc_block(ty, P::object_header_bytes() + len)?;

        if self.blocks[block].header.kind() == BlockHeaderKind::Known(BlockType::Undefined) {
            BlockOps::new(&mut self.medium)
                .set_block_type(block, ty)
                .await?;
            self.blocks[block].header.set_block_type(ty);
        }

        let location = ObjectLocation {
            block,
            offset: self.blocks[block].used_bytes,
        };

        log::trace!("Storage::find_new_object_location({ty:?}, {len}) -> {location:?}");

        Ok(location)
    }
}

fn hash_path(path: &str) -> u32 {
    fxhash::hash32(path.as_bytes())
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
    use super::*;
    use medium::ram::RamStorage;

    const LIPSUM: &[u8] = b"Lorem ipsum dolor sit amet, consectetur adipiscing elit. Fusce in mi scelerisque, porttitor mi amet.";

    pub fn init_test() {
        _ = simple_logger::SimpleLogger::new()
            .with_level(log::LevelFilter::Trace)
            .env()
            .init();
        println!();
    }

    async fn create_fs() -> Storage<RamStorage<256, 32>> {
        init_test();

        let medium = RamStorage::<256, 32>::new();
        Storage::format_and_mount(medium)
            .await
            .expect("Failed to mount storage")
    }

    async fn assert_file_contents(
        storage: &mut Storage<RamStorage<256, 32>>,
        path: &str,
        expected: &[u8],
    ) {
        let mut reader = storage.read(path).await.expect("Failed to open file");

        let mut contents = vec![0; expected.len()];
        let read = reader
            .read(storage, &mut contents)
            .await
            .expect("Failed to read file");

        assert_eq!(0, reader.read(storage, &mut []).await.unwrap());
        assert_eq!(read, expected.len());
        assert_eq!(contents, expected);
    }

    #[async_std::test]
    async fn lookup_returns_error_if_file_does_not_exist() {
        let mut storage = create_fs().await;

        assert!(!storage.exists("foo").await);

        assert!(
            storage.read("foo").await.is_err(),
            "Lookup returned Ok unexpectedly"
        );
    }

    #[async_std::test]
    async fn delete_returns_error_if_file_does_not_exist() {
        let mut storage = create_fs().await;

        assert!(!storage.exists("foo").await);
        storage
            .delete("foo")
            .await
            .expect_err("Delete returned Ok unexpectedly");
    }

    #[async_std::test]
    async fn fails_to_write_file_if_not_enough_space() {
        let mut storage = create_fs().await;

        storage.store("foo", LIPSUM).await.expect("Create failed");

        assert!(storage.exists("foo").await);

        assert!(
            storage.store("bar", LIPSUM).await.is_err(),
            "Lookup returned Ok unexpectedly"
        );
    }

    #[async_std::test]
    async fn written_file_can_be_read() {
        let mut storage = create_fs().await;

        storage
            .store("foo", b"barbaz")
            .await
            .expect("Create failed");

        let mut reader = storage.read("foo").await.expect("Failed to open file");

        let mut buf = [0u8; 6];

        reader
            .read(&mut storage, &mut buf)
            .await
            .expect("Failed to read file");

        assert_eq!(buf, *b"barbaz");
    }

    #[async_std::test]
    async fn reading_overwritten_file_reads_newer_data() {
        let mut storage = create_fs().await;

        storage
            .store("foo", b"barbaz")
            .await
            .expect("Create failed");

        assert!(storage.exists("foo").await);

        storage
            .store("foo", b"foofoobar")
            .await
            .expect("Create failed");

        assert!(storage.exists("foo").await);
        storage.medium.debug_print();

        assert_file_contents(&mut storage, "foo", b"foofoobar").await;
    }

    #[async_std::test]
    async fn content_can_be_longer_than_block_size() {
        let mut storage = create_fs().await;

        storage.store("foo", LIPSUM).await.expect("Create failed");

        storage.medium.debug_print();

        let mut reader = storage.read("foo").await.expect("Failed to open file");

        let mut buf = [0u8; 100];

        // Read in two chunks to test that the reader resumes with the current byte
        reader
            .read(&mut storage, &mut buf[0..50])
            .await
            .expect("Failed to read file");
        reader
            .read(&mut storage, &mut buf[50..])
            .await
            .expect("Failed to read file");

        assert_eq!(buf, *LIPSUM);
    }

    #[async_std::test]
    async fn deleted_file_can_no_longer_be_read() {
        let mut storage = create_fs().await;

        storage
            .store("foo", b"barbaz")
            .await
            .expect("Create failed");

        storage.delete("foo").await.expect("Failed to delete");

        assert!(!storage.exists("foo").await);
        assert!(
            storage.read("foo").await.is_err(),
            "Lookup returned Ok unexpectedly"
        );
    }

    #[async_std::test]
    async fn reading_reads_from_the_correct_file() {
        let mut storage = create_fs().await;

        storage.store("foo", b"bar").await.expect("Create failed");
        storage.store("baz", b"asdf").await.expect("Create failed");

        assert_file_contents(&mut storage, "foo", b"bar").await;
        assert_file_contents(&mut storage, "baz", b"asdf").await;
    }
}
