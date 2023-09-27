use core::ops::{Deref, DerefMut};

use norfs::{medium::cache::ReadCache, Storage, StorageError};
use norfs_esp32s3::{InternalDriver, InternalPartition};

pub struct ConfigPartition;
impl InternalPartition for ConfigPartition {
    const OFFSET: usize = 0x410000;
    const SIZE: usize = 4032 * 1024;
}

type Cache = ReadCache<InternalDriver<ConfigPartition>, 256, 2>;
static mut READ_CACHE: Cache = Cache::new(InternalDriver::new(ConfigPartition));

mod token {
    use core::sync::atomic::{AtomicBool, Ordering};

    static FS_USED: AtomicBool = AtomicBool::new(false);

    pub struct Token(());

    impl Token {
        pub fn take() -> Self {
            let used = FS_USED.fetch_or(true, Ordering::Relaxed);
            assert!(!used);

            debug!("Filesystem token taken");

            Self(())
        }
    }

    impl Drop for Token {
        fn drop(&mut self) {
            debug!("Filesystem token dropped");
            FS_USED.store(false, Ordering::Relaxed);
        }
    }
}

use token::Token;

pub struct FileSystem {
    storage: Storage<&'static mut Cache>,
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

    pub async fn format() {
        let _ = Token::take();

        info!("Formatting storage");
        if let Err(e) = Storage::format(&mut InternalDriver::new(ConfigPartition)).await {
            error!("Failed to format storage: {:?}", e);
        }
    }
}

impl Deref for FileSystem {
    type Target = Storage<&'static mut Cache>;

    fn deref(&self) -> &Self::Target {
        &self.storage
    }
}

impl DerefMut for FileSystem {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.storage
    }
}
