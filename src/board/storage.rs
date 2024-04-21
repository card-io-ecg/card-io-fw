use core::{
    ops::{Deref, DerefMut},
    ptr::addr_of_mut,
};

use macros::partition;
use norfs::{medium::cache::ReadCache, Storage, StorageError};

#[cfg(feature = "esp32s3")]
use norfs_esp32s3 as norfs_impl;

#[cfg(feature = "esp32c6")]
use norfs_esp32c6 as norfs_impl;

use norfs_impl::{InternalDriver, InternalPartition};

#[partition("storage")]
pub struct ConfigPartition;

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

        let cache = unsafe { addr_of_mut!(READ_CACHE).as_mut().unwrap_unchecked() };
        let storage = match Storage::mount(cache).await {
            Ok(storage) => Ok(storage),
            Err(StorageError::NotFormatted) => {
                info!("Formatting storage");
                let cache = unsafe { addr_of_mut!(READ_CACHE).as_mut().unwrap_unchecked() };
                Storage::format_and_mount(cache).await
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
