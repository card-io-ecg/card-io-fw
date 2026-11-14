use core::future::Future;

use alloc::boxed::Box;
#[cfg(feature = "wifi")]
use alloc::vec::Vec;
use config_types::{
    measurement_queue::{measurement_key, measurement_time_key, measurement_version_key, Queue},
    record::{decode_config, encode_config, encode_version, CONFIG_LEN, VERSION_LEN},
    Config,
};
#[cfg(feature = "wifi")]
use ekv::config::MAX_VALUE_SIZE;
use ekv::{CommitError, Database, FormatError, MountError, ReadError, ReadTransaction, WriteError};
use embassy_sync::once_lock::OnceLock;
use embassy_sync_06::blocking_mutex::raw::CriticalSectionRawMutex as EkvRawMutex;
use esp_hal::{
    flash::{Config as FlashConfig, Flash},
    peripherals::FLASH,
    rng::Rng,
};
use log_ring::{decode_next, record_key, waiting, NEXT_KEY};
#[cfg(feature = "wifi")]
use network_services::pairing::SigningKey;

use crate::board::flash::PartitionFlash;

type Store = Database<PartitionFlash, EkvRawMutex>;
type ReadTx<'a> = ReadTransaction<'a, PartitionFlash, EkvRawMutex>;

static STORE: OnceLock<Store> = OnceLock::new();

// ekv requires the keys of one write transaction in ascending order:
// config < device_key < log/NN < log_next < meas/N < queue < time/N < ver/N < version.
const CONFIG_KEY: &[u8] = b"config";
#[cfg(feature = "wifi")]
const DEVICE_KEY: &[u8] = b"device_key";
const VERSION_KEY: &[u8] = b"version";
#[cfg(feature = "wifi")]
const DEVICE_KEY_LEN: usize = 32;
/// The store keeps the newest log records, about 4.1 kB each.
const STORED_LOG_RECORDS: u8 = 32;

/// The sequence numbers of the log records that wait for upload.
pub type WaitingLogs = heapless::Vec<u32, { STORED_LOG_RECORDS as usize }>;

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

/// A buffer that holds the largest measurement, for `load_oldest_measurement`.
#[cfg(feature = "wifi")]
pub fn measurement_buffer() -> Result<Vec<u8>, StorageError> {
    let mut payload = Vec::new();
    reserve_measurement(&mut payload)?;
    Ok(payload)
}

/// `ekv` reads a value only as a whole, and gives no length before the read.
#[cfg(feature = "wifi")]
fn reserve_measurement(payload: &mut Vec<u8>) -> Result<(), StorageError> {
    payload.clear();
    payload
        .try_reserve_exact(MAX_VALUE_SIZE)
        .map_err(|_| StorageError::OutOfMemory)
}

pub fn init(flash: FLASH<'static>, partition: &str) {
    let flash = match Flash::new(flash, FlashConfig::default()) {
        Ok(flash) => flash,
        Err(e) => {
            error!("Failed to initialize flash: {:?}", e);
            return;
        }
    };
    let Some(flash) = PartitionFlash::find(flash, partition) else {
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
    tx.delete(measurement_time_key(queue.head).as_bytes())
        .await?;
    tx.delete(measurement_version_key(queue.head).as_bytes())
        .await?;

    Ok(tx.commit().await?)
}

/// What the store keeps beside the samples of a measurement.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct StoredMeasurement {
    pub version: u8,
    /// The start of the recording in Unix seconds, or `None` when the clock was not set.
    pub started_at: Option<u64>,
}

/// Returns the measurement and the payload length, or `None` if the measurement at `id` is not
/// readable.
#[cfg(feature = "wifi")]
async fn read_measurement(
    store: &Store,
    id: u32,
    payload: &mut [u8],
) -> Result<Option<(StoredMeasurement, usize)>, StorageError> {
    let tx = store.read_transaction().await;

    let mut version = [0; 2];
    let version_key = measurement_version_key(id);
    let Some(1) = read_value(&tx, version_key.as_bytes(), &mut version).await? else {
        return Ok(None);
    };

    // Measurements stored before the clock existed have no time.
    let mut started_at = [0; 8];
    let time_key = measurement_time_key(id);
    let started_at = match read_value(&tx, time_key.as_bytes(), &mut started_at).await? {
        Some(8) => Some(u64::from_le_bytes(started_at)),
        _ => None,
    };

    let measurement = StoredMeasurement {
        version: version[0],
        started_at,
    };
    let key = measurement_key(id);
    match read_value(&tx, key.as_bytes(), payload).await? {
        Some(len @ 1..) => Ok(Some((measurement, len))),
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

    /// A bad record counts as no key. It stays in the store until `save_key` overwrites it.
    #[cfg(feature = "wifi")]
    pub async fn load_key(&mut self) -> Option<SigningKey> {
        let store = self.store;
        let loaded = boxed("load_key", async {
            let mut key = [0; DEVICE_KEY_LEN];
            let tx = store.read_transaction().await;
            let len = read_value(&tx, DEVICE_KEY, &mut key).await?;
            Ok(len.map(|len| (key, len)))
        })
        .await;

        match loaded {
            Ok(None) => None,
            Ok(Some((key, DEVICE_KEY_LEN))) => {
                let key = SigningKey::from_bytes(&key.into()).ok();
                if key.is_none() {
                    warn!("Stored device key is not a valid P-256 scalar");
                }
                key
            }
            Ok(Some((_, len))) => {
                warn!("Stored device key is {} bytes, not {}", len, DEVICE_KEY_LEN);
                None
            }
            Err(e) => {
                warn!("Failed to load the device key: {:?}", e);
                None
            }
        }
    }

    #[cfg(feature = "wifi")]
    pub async fn save_key(&mut self, key: &SigningKey) -> Result<(), StorageError> {
        let store = self.store;
        boxed("save_key", async {
            let mut tx = store.write_transaction().await;
            tx.write(DEVICE_KEY, &key.to_bytes()).await?;

            Ok(tx.commit().await?)
        })
        .await
    }

    #[cfg(feature = "wifi")]
    pub async fn delete_key(&mut self) -> Result<(), StorageError> {
        let store = self.store;
        boxed("delete_key", async {
            let mut tx = store.write_transaction().await;
            tx.delete(DEVICE_KEY).await?;

            Ok(tx.commit().await?)
        })
        .await
    }

    pub async fn store_measurement(
        &mut self,
        measurement: StoredMeasurement,
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
            if let Some(started_at) = measurement.started_at {
                tx.write(
                    measurement_time_key(queue.tail).as_bytes(),
                    &started_at.to_le_bytes(),
                )
                .await?;
            }
            tx.write(
                measurement_version_key(queue.tail).as_bytes(),
                &[measurement.version],
            )
            .await?;

            Ok(tx.commit().await?)
        })
        .await
    }

    /// The sequence number that the next log record gets.
    pub async fn next_log_sequence(&mut self) -> Result<u32, StorageError> {
        let store = self.store;
        boxed("next_log_sequence", async {
            let mut next = [0; 8];
            let tx = store.read_transaction().await;
            let len = read_value(&tx, NEXT_KEY, &mut next).await?;

            Ok(decode_next(len.map(|len| &next[..len])))
        })
        .await
    }

    pub async fn store_log_record(
        &mut self,
        sequence: u32,
        record: &[u8],
    ) -> Result<(), StorageError> {
        let store = self.store;
        boxed("store_log_record", async {
            let mut tx = store.write_transaction().await;
            tx.write(&record_key(sequence, STORED_LOG_RECORDS), record)
                .await?;
            tx.write(NEXT_KEY, &sequence.wrapping_add(1).to_le_bytes())
                .await?;

            Ok(tx.commit().await?)
        })
        .await
    }

    /// The sequence numbers of the log records that wait for upload, oldest first.
    pub async fn waiting_log_records(&mut self) -> Result<WaitingLogs, StorageError> {
        let store = self.store;
        boxed("waiting_log_records", async {
            let mut next = [0; 8];
            let tx = store.read_transaction().await;
            let len = read_value(&tx, NEXT_KEY, &mut next).await?;
            let next = decode_next(len.map(|len| &next[..len]));

            let mut sequences = heapless::Vec::new();
            for sequence in waiting(next, STORED_LOG_RECORDS) {
                // An empty buffer finds the record without reading it.
                let found = match tx
                    .read(&record_key(sequence, STORED_LOG_RECORDS), &mut [])
                    .await
                {
                    Ok(_) | Err(ReadError::BufferTooSmall) => true,
                    Err(ReadError::KeyNotFound) => false,
                    Err(e) => return Err(e.into()),
                };
                if found {
                    // `waiting` gives at most `STORED_LOG_RECORDS` numbers.
                    unwrap!(sequences.push(sequence).ok());
                }
            }
            Ok(sequences)
        })
        .await
    }

    /// Reads the log record of `sequence` into `record` and returns its length. `None` when the
    /// slot holds no record.
    #[cfg(feature = "wifi")]
    pub async fn load_log_record(
        &mut self,
        sequence: u32,
        record: &mut [u8],
    ) -> Result<Option<usize>, StorageError> {
        let store = self.store;
        boxed("load_log_record", async {
            let tx = store.read_transaction().await;
            read_value(&tx, &record_key(sequence, STORED_LOG_RECORDS), record).await
        })
        .await
    }

    #[cfg(feature = "wifi")]
    pub async fn delete_log_record(&mut self, sequence: u32) -> Result<(), StorageError> {
        let store = self.store;
        boxed("delete_log_record", async {
            let mut tx = store.write_transaction().await;
            tx.delete(&record_key(sequence, STORED_LOG_RECORDS)).await?;

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

    /// Loads into `payload` and returns the format version. `payload` keeps its capacity, so one
    /// buffer from `measurement_buffer` serves every load.
    #[cfg(feature = "wifi")]
    pub async fn load_oldest_measurement(
        &mut self,
        payload: &mut Vec<u8>,
    ) -> Result<Option<StoredMeasurement>, StorageError> {
        let store = self.store;
        boxed("load_oldest_measurement", async {
            reserve_measurement(payload)?;
            payload.resize(MAX_VALUE_SIZE, 0);

            loop {
                let queue = read_queue(store).await?;
                if queue.is_empty() {
                    return Ok(None);
                }

                if let Some((measurement, len)) =
                    read_measurement(store, queue.head, payload).await?
                {
                    payload.truncate(len);
                    return Ok(Some(measurement));
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
