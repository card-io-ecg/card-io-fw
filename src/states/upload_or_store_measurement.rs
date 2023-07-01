use core::fmt::Write;

use norfs::{writer::FileDataWriter, OnCollision, StorageError};
use signal_processing::{buffer::Buffer, i24::i24};

use crate::{board::initialized::Board, states::BIG_OBJECTS, AppState};

/// Whether to store the measurement or not. Used instead of a bool to reduce confusion.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum StoreMeasurement {
    Store,
    DontStore,
}

pub async fn upload_or_store_measurement(board: &mut Board, next_state: AppState) -> AppState {
    let ecg = unsafe { BIG_OBJECTS.as_ecg() };

    if try_to_upload(board, &mut ecg.buffer).await == StoreMeasurement::Store {
        if let Err(e) = try_store_measurement(board, &mut ecg.buffer).await {
            log::error!("Failed to store measurement: {e:?}");
        }
    }

    next_state
}

async fn try_to_upload<const N: usize>(
    board: &mut Board,
    buffer: &mut Buffer<i24, N>,
) -> StoreMeasurement {
    if buffer.len() < buffer.capacity() / 2 {
        log::debug!("Buffer is too short to upload.");
        // We don't want to store too-short measurements.
        return StoreMeasurement::DontStore;
    }

    // TODO: scan/connection shouldn't be here.
    let is_connected = false;
    // If we're not connected, look around for a network to connect to.
    if !is_connected {
        if board.config.known_networks.is_empty() {
            // We don't have networks configured. Best we can do is store the measurement.
            return StoreMeasurement::Store;
        }

        // TODO: scan
        // If we found a network, attempt to upload.
        // TODO: only try to upload if we are registered.
        // If we could not upload, save to file.
    }

    log::debug!("Trying to upload measurement");
    // TODO

    StoreMeasurement::DontStore
}

async fn try_store_measurement<const N: usize>(
    board: &mut Board,
    measurement: &mut Buffer<i24, N>,
) -> Result<(), StorageError> {
    log::debug!("Trying to store measurement");

    let Some(storage) = &mut board.storage else {
        return Ok(());
    };

    let mut max_index = None;
    let mut dir = storage.read_dir().await?;
    let mut buffer = [0; 64];
    while let Some(file) = dir.next(storage).await? {
        match file.name(storage, &mut buffer).await {
            Ok(name) => {
                if let Some(idx) = name
                    .strip_prefix("meas.")
                    .and_then(|s| s.parse::<u32>().ok())
                {
                    let update_max = if let Some(max) = max_index {
                        idx > max
                    } else {
                        true
                    };

                    if update_max {
                        max_index = Some(idx);
                    }
                }
            }
            Err(StorageError::InsufficientBuffer) => {
                // not a measurement file, ignore
            }
            Err(e) => {
                log::warn!("Failed to read file name: {:?}", e);
                return Err(e);
            }
        }
    }

    let mut filename = heapless::String::<16>::new();
    if write!(
        &mut filename,
        "meas.{}",
        max_index.map(|idx| idx + 1).unwrap_or(0)
    )
    .is_err()
    {
        log::warn!("Failed to create filename");
        return Ok(());
    }

    storage
        .store_writer(
            &filename,
            &MeasurementWriter(measurement),
            OnCollision::Fail,
        )
        .await?;

    measurement.clear();

    log::info!("Measurement saved to {filename}");

    Ok(())
}

struct MeasurementWriter<'a, const N: usize>(&'a Buffer<i24, N>);

impl<const N: usize> FileDataWriter for MeasurementWriter<'_, N> {
    async fn write<M>(
        &self,
        writer: &mut norfs::writer::Writer<M>,
        storage: &mut norfs::Storage<M>,
    ) -> Result<(), StorageError>
    where
        M: norfs::medium::StorageMedium,
        [(); M::BLOCK_COUNT]:,
    {
        let buffers = self.0.as_bytes();

        let mut writer = writer.bind(storage);

        writer.write_all(buffers.0).await?;
        writer.write_all(buffers.1).await?;

        Ok(())
    }

    fn estimate_length(&self) -> usize {
        let buffers = self.0.as_bytes();

        buffers.0.len() + buffers.0.len()
    }
}
