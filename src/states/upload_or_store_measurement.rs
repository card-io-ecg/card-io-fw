use core::mem::{self, MaybeUninit};

use alloc::{boxed::Box, vec::Vec};
use embassy_net::{
    dns::DnsSocket,
    tcp::client::{TcpClient, TcpClientState},
};
use embassy_time::{Duration, Timer};
use embedded_nal_async::{Dns, TcpConnect};
use norfs::{medium::StorageMedium, writer::FileDataWriter, OnCollision, Storage, StorageError};
use reqwless::{client::HttpClient, request::RequestBuilder};
use signal_processing::compressing_buffer::CompressingBuffer;
use ufmt::uwrite;

use crate::{
    board::{
        initialized::{Board, StaMode},
        wifi::sta::ConnectionState,
    },
    states::display_message,
    AppState, SerialNumber,
};

/// Whether to store the measurement or not. Used instead of a bool to reduce confusion.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum StoreMeasurement {
    Store,
    DontStore,
}

pub async fn upload_or_store_measurement<const SIZE: usize>(
    board: &mut Board,
    mut buffer: Box<CompressingBuffer<SIZE>>,
    next_state: AppState,
) -> AppState {
    if try_to_upload(board, &mut buffer).await == StoreMeasurement::Store {
        let store_result = try_store_measurement(board, &mut buffer).await;
        buffer.clear();

        if let Err(e) = store_result {
            display_message(board, "Could not store measurement").await;
            error!("Failed to store measurement: {:?}", e);

            Timer::after(Duration::from_secs(2)).await;
        }
    } else {
        // Drop to free up 90kB of memory.
        mem::drop(buffer);

        upload_stored(board).await;
    }

    next_state
}

struct Resources {
    //request_buffer: [u8; 2048],
    client_state: TcpClientState<1, 1024, 1024>,
    rx_buffer: [u8; 1024],
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

    let mut uploading_msg = heapless::String::<48>::new();
    unwrap!(uwrite!(
        &mut uploading_msg,
        "Uploading measurement: {} bytes",
        buffer.byte_count()
    ));

    display_message(board, uploading_msg.as_str()).await;
    let samples = buffer.make_contiguous();

    let mut resources = Box::new(Resources {
        client_state: TcpClientState::new(),
        rx_buffer: [0; 1024],
    });

    let client = TcpClient::new(sta.stack(), &resources.client_state);
    let dns = DnsSocket::new(sta.stack());

    let mut client = HttpClient::new(&client, &dns);
    match upload_measurement(
        &board.config.backend_url,
        &mut client,
        0,
        samples,
        &mut resources.rx_buffer,
    )
    .await
    {
        Ok(_) => {
            // Upload successful, do not store in file.
            StoreMeasurement::DontStore
        }
        Err(_) => StoreMeasurement::Store,
    }
}

async fn upload_stored(board: &mut Board) {
    board.signal_sta_work_available();
    let sta = if !board.config.known_networks.is_empty() {
        // This call should handle the case where there are no files stored.
        board.enable_wifi_sta(StaMode::OnDemand).await
    } else {
        board.disable_wifi().await;
        None
    };

    let Some(sta) = sta else {
        return;
    };

    if sta.connection_state() != ConnectionState::Connected {
        while sta.wait_for_state_change().await == ConnectionState::Connecting {
            display_message(board, "Connecting...").await;
        }

        if sta.connection_state() != ConnectionState::Connected {
            // If we do not have a network connection, save to file.
            return;
        }
    }

    display_message(board, "Uploading stored measurements...").await;

    let Some(storage) = board.storage.as_mut() else {
        return;
    };

    let Ok(mut dir) = storage.read_dir().await else {
        return;
    };

    let mut fn_buffer = [0; 64];

    let mut resources = Box::new(Resources {
        client_state: TcpClientState::new(),
        rx_buffer: [0; 1024],
    });

    let client = TcpClient::new(sta.stack(), &resources.client_state);
    let dns = DnsSocket::new(sta.stack());

    let mut client = HttpClient::new(&client, &dns);

    loop {
        match dir.next(storage).await {
            Ok(file) => {
                let Some(file) = file else {
                    return;
                };

                match file.name(storage, &mut fn_buffer).await {
                    Ok(name) => {
                        if name.starts_with("meas.") {
                            let Ok(size) = file.size(storage).await else {
                                warn!("Failed to read size of {}", name);
                                continue;
                            };

                            let Ok(mut buffer) = buffer_with_capacity(size, 0) else {
                                warn!("Failed to allocate {} bytes for {}", size, name);
                                continue;
                            };

                            let mut reader = file.open().await;
                            if let Err(e) = reader.read_all(storage, buffer.as_mut()).await {
                                warn!("Failed to read {}: {:?}", name, e);
                                continue;
                            };

                            if let Err(e) = upload_measurement(
                                &board.config.backend_url,
                                &mut client,
                                0,
                                buffer.as_ref(),
                                &mut resources.rx_buffer,
                            )
                            .await
                            {
                                warn!("Failed to upload {}: {:?}", name, e);
                                return;
                            }
                        }
                    }
                    Err(StorageError::InsufficientBuffer) => {
                        // not a measurement file, ignore
                    }
                    Err(e) => {
                        warn!("Failed to read file name: {:?}", e);
                        return;
                    }
                }
            }
            Err(e) => {
                warn!("Failed to read directory: {:?}", e);
                return;
            }
        }
    }
}

fn buffer_with_capacity<T: Copy>(size: usize, init_val: T) -> Result<Box<[T]>, ()> {
    let mut buffer = Vec::new();

    if buffer.try_reserve_exact(size).is_err() {
        return Err(());
    }

    unsafe {
        let uninit = buffer.spare_capacity_mut();
        uninit.fill(MaybeUninit::new(init_val));
        let len = uninit.len();
        buffer.set_len(len);
    }

    Ok(buffer.into_boxed_slice())
}

async fn upload_measurement<T, DNS>(
    url: &str,
    client: &mut HttpClient<'_, T, DNS>,
    meas_timestamp: u64,
    samples: &[u8],
    rx_buffer: &mut [u8],
) -> Result<(), ()>
where
    T: TcpConnect,
    DNS: Dns,
{
    let mut resource = match client.resource(url).await {
        Ok(res) => res,
        Err(e) => {
            warn!("HTTP error: {}", e);
            return Err(());
        }
    };

    let mut path = heapless::String::<32>::new();
    unwrap!(uwrite!(&mut path, "/upload_data/{}", SerialNumber::new()));

    let mut timestamp = heapless::String::<32>::new();
    unwrap!(uwrite!(&mut timestamp, "{}", meas_timestamp));

    let response = resource
        .post(&path)
        .headers(&[("X-Timestamp", timestamp.as_str())]) // TODO
        .body(samples)
        .send(rx_buffer)
        .await;

    if let Err(e) = response {
        warn!("HTTP error: {:?}", e);
        return Err(());
    }

    Ok(())
}

async fn try_store_measurement<const SIZE: usize>(
    board: &mut Board,
    measurement: &mut CompressingBuffer<SIZE>,
) -> Result<(), StorageError> {
    debug!("Trying to store measurement");

    let mut saving_msg = heapless::String::<48>::new();
    unwrap!(uwrite!(
        &mut saving_msg,
        "Saving measurement: {} bytes",
        measurement.byte_count()
    ));
    display_message(board, &saving_msg).await;
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
