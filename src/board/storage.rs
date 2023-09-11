use core::{
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicBool, Ordering},
};

use norfs::{
    drivers::internal::{InternalDriver, InternalPartition},
    medium::cache::ReadCache,
    Storage, StorageError,
};

pub struct ConfigPartition;
impl InternalPartition for ConfigPartition {
    const OFFSET: usize = 0x410000;
    const SIZE: usize = 4032 * 1024;
}

static mut READ_CACHE: ReadCache<InternalDriver<ConfigPartition>, 256, 2> =
    ReadCache::new(InternalDriver::new(ConfigPartition));

static FS_USED: AtomicBool = AtomicBool::new(false);

struct Token;
impl Token {
    fn take() -> Self {
        let used = FS_USED.fetch_or(true, Ordering::Relaxed);
        assert!(!used);

        Self
    }
}

impl Drop for Token {
    fn drop(&mut self) {
        FS_USED.store(false, Ordering::Relaxed);
    }
}

pub struct FileSystem {
    storage: Storage<&'static mut ReadCache<InternalDriver<ConfigPartition>, 256, 2>>,
    _token: Token,
}

impl FileSystem {
    pub async fn mount() -> Option<Self> {
        let token = Token::take();

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
            Ok(storage) => Some(Self {
                storage,
                _token: token,
            }),
            Err(e) => {
                error!("Failed to mount storage: {:?}", e);
                None
            }
        }
    }
}

impl Deref for FileSystem {
    type Target = Storage<&'static mut ReadCache<InternalDriver<ConfigPartition>, 256, 2>>;

    fn deref(&self) -> &Self::Target {
        &self.storage
    }
}

impl DerefMut for FileSystem {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.storage
    }
}
