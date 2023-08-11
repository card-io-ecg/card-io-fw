use core::fmt::Write;

use norfs::{writer::FileDataWriter, OnCollision, StorageError};
use signal_processing::compressing_buffer::CompressingBuffer;

use crate::{
    board::{
        initialized::{Board, StaMode},
        wifi::sta::ConnectionState,
    },
    AppState,
};

/// Whether to store the measurement or not. Used instead of a bool to reduce confusion.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum StoreMeasurement {
    Store,
    DontStore,
}

pub async fn upload_or_store_measurement<const SIZE: usize>(
    board: &mut Board,
    buffer: &mut CompressingBuffer<SIZE>,
    next_state: AppState,
) -> AppState {
    if try_to_upload(board, buffer).await == StoreMeasurement::Store {
        if let Err(e) = try_store_measurement(board, buffer).await {
            error!("Failed to store measurement: {:?}", e);
        }
    }

    buffer.clear();

    next_state
}

async fn try_to_upload<const SIZE: usize>(
    board: &mut Board,
    buffer: &mut CompressingBuffer<SIZE>,
) -> StoreMeasurement {
    const SAMPLE_RATE: usize = 1000; // samples/sec
    if buffer.len() < 20 * SAMPLE_RATE {
        debug!("Buffer is too short to upload.");
        // We don't want to store too-short measurements.
        return StoreMeasurement::DontStore;
    }

    board.signal_sta_work_available();
    let sta = if !board.config.known_networks.is_empty() {
        board.enable_wifi_sta(StaMode::OnDemand).await
    } else {
        board.disable_wifi().await;
        None
    };

    let Some(sta) = sta else {
        return StoreMeasurement::Store;
    };

    // TODO: we need to wait for the wifi to connect, or for the stack to conclude that connection
    // is not possible.

    if sta.connection_state() != ConnectionState::Connected {
        // If we do not have a network connection, save to file.
        return StoreMeasurement::Store;
    }

    // If we found a network, attempt to upload.
    // TODO: only try to upload if we are registered.
    debug!("Trying to upload measurement");
    // TODO

    // Upload successful, do not store in file.
    StoreMeasurement::DontStore
}

async fn try_store_measurement<const SIZE: usize>(
    board: &mut Board,
    measurement: &mut CompressingBuffer<SIZE>,
) -> Result<(), StorageError> {
    debug!("Trying to store measurement");

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
                warn!("Failed to read file name: {:?}", e);
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
        warn!("Failed to create filename");
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

    info!("Measurement saved to {}", filename);

    Ok(())
}

struct MeasurementWriter<'a, const N: usize>(&'a CompressingBuffer<N>);

impl<'a, const N: usize> MeasurementWriter<'a, N> {
    // We're good with a straight u8 until 127 samples, then we can consider switching to varint.
    const FORMAT_VERSION: u8 = 0;
}

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
        // Here we only store differences, but not the initial sample. The DC offset does not
        // matter for the analysis, and we can reconstruct everything else from the differences.
        let buffers = self.0.as_slices();

        let mut writer = writer.bind(storage);

        writer
            .write_all(&Self::FORMAT_VERSION.to_le_bytes())
            .await?;
        writer.write_all(buffers.0).await?;
        writer.write_all(buffers.1).await?;

        Ok(())
    }

    fn estimate_length(&self) -> usize {
        let buffers = self.0.as_slices();

        Self::FORMAT_VERSION.to_le_bytes().len() + buffers.0.len() + buffers.1.len()
    }
}
