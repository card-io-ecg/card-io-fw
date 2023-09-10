use alloc::boxed::Box;
use embassy_net::{
    dns::DnsSocket,
    tcp::client::{TcpClient, TcpClientState},
};
use embassy_time::{Duration, Timer};
use norfs::{medium::StorageMedium, writer::FileDataWriter, OnCollision, Storage, StorageError};
use reqwless::client::HttpClient;
use signal_processing::compressing_buffer::CompressingBuffer;
use ufmt::uwrite;

use crate::{
    board::{
        initialized::{Board, StaMode},
        wifi::sta::ConnectionState,
    },
    states::display_message,
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
            display_message(board, "Could not store measurement").await;
            error!("Failed to store measurement: {:?}", e);

            Timer::after(Duration::from_secs(2)).await;
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
        debug!("Buffer is too short to upload or store.");
        // We don't want to store too-short measurements.
        return StoreMeasurement::DontStore;
    }

    if board.config.backend_url.is_empty() {
        debug!("No backend URL configured, not uploading.");
        return StoreMeasurement::Store;
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

    if sta.connection_state() != ConnectionState::Connected {
        while sta.wait_for_state_change().await == ConnectionState::Connecting {
            display_message(board, "Connecting...").await;
        }

        if sta.connection_state() != ConnectionState::Connected {
            // If we do not have a network connection, save to file.
            return StoreMeasurement::Store;
        }
    }

    // If we found a network, attempt to upload.
    // TODO: only try to upload if we are registered.
    debug!("Trying to upload measurement");
    struct Resources {
        //request_buffer: [u8; 2048],
        client_state: TcpClientState<1, 1024, 1024>,
    }

    let resources = Box::new(Resources {
        //request_buffer: [0; 2048],
        client_state: TcpClientState::new(),
    });

    let client = TcpClient::new(sta.stack(), &resources.client_state);
    let dns = DnsSocket::new(sta.stack());

    let _client = HttpClient::new(&client, &dns);

    // TODO

    // Upload successful, do not store in file.
    StoreMeasurement::DontStore
}

async fn try_store_measurement<const SIZE: usize>(
    board: &mut Board,
    measurement: &mut CompressingBuffer<SIZE>,
) -> Result<(), StorageError> {
    debug!("Trying to store measurement");

    display_message(board, "Saving measurement...").await;
    let Some(storage) = board.storage.as_mut() else {
        return Ok(());
    };

    let timeout = Timer::after(Duration::from_secs(2));

    let meas_idx = find_measurement_index(storage).await?;

    let mut filename = heapless::String::<16>::new();
    unwrap!(uwrite!(&mut filename, "meas.{}", meas_idx));

    storage
        .store_writer(
            &filename,
            &MeasurementWriter(measurement),
            OnCollision::Fail,
        )
        .await?;

    measurement.clear();

    info!("Measurement saved to {}", filename);

    timeout.await;

    Ok(())
}

async fn find_measurement_index<M>(storage: &mut Storage<M>) -> Result<u32, StorageError>
where
    M: StorageMedium,
    [(); M::BLOCK_COUNT]:,
{
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

    Ok(max_index.map(|idx| idx + 1).unwrap_or(0))
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
        storage: &mut Storage<M>,
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
