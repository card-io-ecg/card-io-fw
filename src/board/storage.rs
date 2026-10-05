use core::future::Future;

use alloc::boxed::Box;
use config_types::{
    measurement_queue::{measurement_key, measurement_version_key, Queue},
    record::{decode_config, encode_config, encode_version, CONFIG_LEN, VERSION_LEN},
    Config,
};
use ekv::{CommitError, Database, FormatError, MountError, ReadError, ReadTransaction, WriteError};
use embassy_sync::once_lock::OnceLock;
use embassy_sync_06::blocking_mutex::raw::CriticalSectionRawMutex as EkvRawMutex;
use esp_hal::{peripherals::FLASH, rng::Rng};
use esp_storage::FlashStorage;

use crate::board::flash::PartitionFlash;

type Store = Database<PartitionFlash, EkvRawMutex>;
type ReadTx<'a> = ReadTransaction<'a, PartitionFlash, EkvRawMutex>;

static STORE: OnceLock<Store> = OnceLock::new();

// ekv requires the keys of one write transaction in ascending order:
// config < meas/N < queue < ver/N < version.
const CONFIG_KEY: &[u8] = b"config";
const VERSION_KEY: &[u8] = b"version";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum StorageError {
    Flash,
    Corrupted,
    Full,
    OutOfMemory,
}

impl<E> From<MountError<E>> for StorageError {
    fn from(error: MountError<E>) -> Self {
        match error {
            MountError::Flash(_) => Self::Flash,
            MountError::Corrupted => Self::Corrupted,
        }
    }
}

impl<E> From<FormatError<E>> for StorageError {
    fn from(_: FormatError<E>) -> Self {
        Self::Flash
    }
}

impl<E> From<ReadError<E>> for StorageError {
    fn from(error: ReadError<E>) -> Self {
        match error {
            ReadError::Flash(_) => Self::Flash,
            _ => Self::Corrupted,
        }
    }
}

impl<E> From<WriteError<E>> for StorageError {
    fn from(error: WriteError<E>) -> Self {
        match error {
            WriteError::Flash(_) => Self::Flash,
            WriteError::Full => Self::Full,
            _ => Self::Corrupted,
        }
    }
}

impl<E> From<CommitError<E>> for StorageError {
    fn from(error: CommitError<E>) -> Self {
        match error {
            CommitError::Flash(_) => Self::Flash,
            _ => Self::Corrupted,
        }
    }
}

#[cfg(feature = "wifi")]
pub struct Measurement {
    pub version: u8,
    pub payload: alloc::vec::Vec<u8>,
}

pub fn init(flash: FLASH<'static>, partition: &str) {
    let Some(flash) = PartitionFlash::find(FlashStorage::new(flash), partition) else {
        return;
    };

    info!(
        "storage: Database is {} bytes",
        core::mem::size_of::<Store>()
    );

    let mut config = ekv::Config::default();
    config.random_seed = Rng::new().random();
    STORE.get_or_init(|| Database::new(flash, config));
}

#[cfg(feature = "wifi")]
pub async fn lock_flash() -> Option<impl core::ops::DerefMut<Target = PartitionFlash>> {
    Some(STORE.try_get()?.lock_flash().await)
}

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

/// Runs an ekv future on the heap, so that it does not take space in the caller's future.
async fn boxed<T, F: Future<Output = Result<T, StorageError>>>(
    name: &'static str,
    future: F,
) -> Result<T, StorageError> {
    debug!(
        "storage: {} future is {} bytes",
        name,
        core::mem::size_of::<F>()
    );
    let future = Box::try_new(future).map_err(|_| StorageError::OutOfMemory)?;
    Box::into_pin(future).await
}

async fn read_value(
    tx: &ReadTx<'_>,
    key: &[u8],
    buffer: &mut [u8],
) -> Result<Option<usize>, StorageError> {
    match tx.read(key, buffer).await {
        Ok(len) => Ok(Some(len)),
        Err(ReadError::KeyNotFound) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

async fn read_queue(store: &Store) -> Result<Queue, StorageError> {
    let mut buffer = [0; 8];
    let tx = store.read_transaction().await;

    match read_value(&tx, Queue::KEY, &mut buffer).await? {
        Some(len) => Queue::decode(&buffer[..len]).map_err(|_| StorageError::Corrupted),
        None => Ok(Queue::default()),
    }
}

#[cfg(feature = "wifi")]
async fn pop_measurement(store: &Store, queue: Queue) -> Result<(), StorageError> {
    let mut tx = store.write_transaction().await;
    tx.delete(measurement_key(queue.head).as_bytes()).await?;
    tx.write(Queue::KEY, &queue.after_pop().encode()).await?;
    tx.delete(measurement_version_key(queue.head).as_bytes())
        .await?;

    Ok(tx.commit().await?)
}

/// Returns the version and the payload length, or `None` if the measurement at `id` is not
/// readable.
#[cfg(feature = "wifi")]
async fn read_measurement(
    store: &Store,
    id: u32,
    payload: &mut [u8],
) -> Result<Option<(u8, usize)>, StorageError> {
    let tx = store.read_transaction().await;

    let mut version = [0; 2];
    let version_key = measurement_version_key(id);
    let Some(1) = read_value(&tx, version_key.as_bytes(), &mut version).await? else {
        return Ok(None);
    };

    let key = measurement_key(id);
    match read_value(&tx, key.as_bytes(), payload).await? {
        Some(len @ 1..) => Ok(Some((version[0], len))),
        _ => Ok(None),
    }
}

pub struct FileSystem {
    store: &'static Store,
    _token: Token,
}

impl FileSystem {
    pub async fn mount() -> Option<Self> {
        let token = Token::take();
        let store = STORE.try_get()?;

        let mounted = boxed("mount", async {
            match store.mount().await {
                Err(MountError::Corrupted) => {
                    info!("Formatting storage");
                    store.format().await?;
                    Ok(store.mount().await?)
                }
                result => Ok(result?),
            }
        })
        .await;

        match mounted {
            Ok(()) => Some(Self {
                store,
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
        let Some(store) = STORE.try_get() else {
            return;
        };

        info!("Formatting storage");
        if let Err(e) = boxed("format", async { Ok(store.format().await?) }).await {
            error!("Failed to format storage: {:?}", e);
        }
    }

    pub async fn load_config(&mut self) -> Config {
        let store = self.store;
        let loaded = boxed("load_config", async {
            let mut version = [0; VERSION_LEN];
            let mut config = [0; CONFIG_LEN];

            let tx = store.read_transaction().await;
            let version_len = read_value(&tx, VERSION_KEY, &mut version).await?;
            let config_len = read_value(&tx, CONFIG_KEY, &mut config).await?;

            Ok(decode_config(
                version_len.map(|len| &version[..len]),
                config_len.map(|len| &config[..len]),
            ))
        })
        .await;

        match loaded {
            Ok(Ok(config)) => config,
            Ok(Err(e)) => {
                warn!("Stored config is unusable: {:?}. Reverting to defaults", e);
                Config::default()
            }
            Err(e) => {
                warn!("Failed to load config: {:?}. Reverting to defaults", e);
                Config::default()
            }
        }
    }

    pub async fn save_config(&mut self, config: &Config) -> Result<(), StorageError> {
        let store = self.store;
        boxed("save_config", async {
            let mut version = [0; VERSION_LEN];
            let mut encoded = [0; CONFIG_LEN];
            let version = encode_version(&mut version).map_err(|_| StorageError::Corrupted)?;
            let encoded =
                encode_config(config, &mut encoded).map_err(|_| StorageError::Corrupted)?;

            let mut tx = store.write_transaction().await;
            tx.write(CONFIG_KEY, encoded).await?;
            tx.write(VERSION_KEY, version).await?;

            Ok(tx.commit().await?)
        })
        .await
    }

    pub async fn store_measurement(
        &mut self,
        version: u8,
        payload: &[u8],
    ) -> Result<(), StorageError> {
        let store = self.store;
        boxed("store_measurement", async {
            let queue = read_queue(store).await?;
            let pushed = queue.after_push().map_err(|_| StorageError::Full)?;

            let mut tx = store.write_transaction().await;
            tx.write(measurement_key(queue.tail).as_bytes(), payload)
                .await?;
            tx.write(Queue::KEY, &pushed.encode()).await?;
            tx.write(measurement_version_key(queue.tail).as_bytes(), &[version])
                .await?;

            Ok(tx.commit().await?)
        })
        .await
    }

    #[cfg(feature = "wifi")]
    pub async fn has_measurements(&mut self) -> Result<bool, StorageError> {
        let store = self.store;
        boxed("has_measurements", async {
            Ok(!read_queue(store).await?.is_empty())
        })
        .await
    }

    pub async fn measurement_count(&mut self) -> Result<u32, StorageError> {
        let store = self.store;
        boxed("measurement_count", async {
            Ok(read_queue(store).await?.len())
        })
        .await
    }

    #[cfg(feature = "wifi")]
    pub async fn load_oldest_measurement(&mut self) -> Result<Option<Measurement>, StorageError> {
        let store = self.store;
        boxed("load_oldest_measurement", async {
            let mut payload = alloc::vec::Vec::new();
            payload
                .try_reserve_exact(ekv::config::MAX_VALUE_SIZE)
                .map_err(|_| StorageError::OutOfMemory)?;
            payload.resize(ekv::config::MAX_VALUE_SIZE, 0);

            loop {
                let queue = read_queue(store).await?;
                if queue.is_empty() {
                    return Ok(None);
                }

                if let Some((version, len)) =
                    read_measurement(store, queue.head, &mut payload).await?
                {
                    payload.truncate(len);
                    return Ok(Some(Measurement { version, payload }));
                }

                warn!("Dropping unreadable measurement {}", queue.head);
                pop_measurement(store, queue).await?;
            }
        })
        .await
    }

    #[cfg(feature = "wifi")]
    pub async fn delete_oldest_measurement(&mut self) -> Result<(), StorageError> {
        let store = self.store;
        boxed("delete_oldest_measurement", async {
            let queue = read_queue(store).await?;
            if queue.is_empty() {
                return Ok(());
            }

            pop_measurement(store, queue).await
        })
        .await
    }
}
