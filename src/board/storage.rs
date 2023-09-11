use norfs::{
    drivers::internal::{InternalDriver, InternalPartition},
    medium::cache::ReadCache,
    Storage, StorageError,
};
use static_cell::make_static;

pub struct ConfigPartition;
impl InternalPartition for ConfigPartition {
    const OFFSET: usize = 0x410000;
    const SIZE: usize = 4032 * 1024;
}

static mut READ_CACHE: ReadCache<InternalDriver<ConfigPartition>, 256, 2> =
    ReadCache::new(InternalDriver::new(ConfigPartition));

pub async fn setup_storage(
) -> Option<&'static mut Storage<&'static mut ReadCache<InternalDriver<ConfigPartition>, 256, 2>>> {
    unsafe { READ_CACHE = ReadCache::new(InternalDriver::new(ConfigPartition)) };
    let storage = match Storage::mount(unsafe { &mut READ_CACHE }).await {
        Ok(storage) => Ok(storage),
        Err(StorageError::NotFormatted) => {
            info!("Formatting storage");
            Storage::format_and_mount(unsafe { &mut READ_CACHE }).await
        }
        e => e,
    };

    match storage {
        Ok(storage) => Some(make_static!(storage)),
        Err(e) => {
            error!("Failed to mount storage: {:?}", e);
            None
        }
    }
}
