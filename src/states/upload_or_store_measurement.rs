use core::{
    marker::PhantomData,
    mem::{self, MaybeUninit},
};

use alloc::{boxed::Box, vec::Vec};
use embassy_futures::select::{select, Either};
use embassy_net::dns::DnsSocket;
use embassy_time::{Duration, Timer};
use embedded_menu::items::NavigationItem;
use embedded_nal_async::{Dns, TcpConnect};
use gui::screens::create_menu;
use norfs::{
    medium::StorageMedium, read_dir::DirEntry, writer::FileDataWriter, OnCollision, Storage,
    StorageError,
};
use reqwless::{
    client::HttpClient,
    request::{Method, RequestBody, RequestBuilder},
    response::Status,
};
use signal_processing::compressing_buffer::{CompressingBuffer, EkgFormat};
use ufmt::uwrite;

use crate::{
    board::{
        initialized::{Board, StaMode},
        wait_for_connection, HttpClientResources,
    },
    buffered_tcp_client::BufferedTcpClient,
    states::{
        display_menu_screen, display_message, menu::storage::MeasurementAction, MenuEventHandler,
    },
    AppState, SerialNumber,
};

/// Whether to store the measurement or not. Used instead of a bool to reduce confusion.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
enum StoreMeasurement {
    Store,
    DontStore,
}

pub async fn upload_stored_measurements(board: &mut Board, next_state: AppState) -> AppState {
    upload_stored(board).await;

    next_state
}

pub async fn upload_or_store_measurement<const SIZE: usize>(
    board: &mut Board,
    mut buffer: Box<CompressingBuffer<SIZE>>,
    next_state: AppState,
) -> AppState {
    let sample_count = buffer.len();
    let samples = buffer.make_contiguous();

    const SAMPLE_RATE: usize = 1000; // samples/sec

    debug!("Measurement length: {} samples", sample_count);

    if sample_count < 20 * SAMPLE_RATE {
        // We don't want to store too-short measurements.
        debug!("Measurement is too short to upload or store.");
        display_message(board, "Measurement too short, discarding").await;
        return next_state;
    }

    let (can_upload, can_store) = match board.config.measurement_action {
        MeasurementAction::Ask => ask_for_measurement_action(board).await,
        MeasurementAction::Auto => (true, true),
        MeasurementAction::Store => (false, true),
        MeasurementAction::Upload => (true, false),
        MeasurementAction::Discard => (false, false),
    };

    let store_after_upload = if can_upload {
        let upload_result = try_to_upload(board, samples).await;
        debug!("Upload result: {:?}", upload_result);
        upload_result == StoreMeasurement::Store
    } else {
        true
    };

    if can_store && store_after_upload {
        let store_result = try_store_measurement(board, samples).await;

        if let Err(e) = store_result {
            display_message(board, "Could not store measurement").await;
            error!("Failed to store measurement: {:?}", e);
        }
    }

    // Only upload if we did not store.
    if can_upload && !store_after_upload {
        // Drop to free up 90kB of memory.
        mem::drop(buffer);
        upload_stored(board).await;
    }

    next_state
}

#[derive(Default)]
struct ReturnEvent<R>(PhantomData<R>);

impl<R> MenuEventHandler for ReturnEvent<R> {
    type Input = R;
    type Result = R;

    async fn handle_event(
        &mut self,
        event: Self::Input,
        _board: &mut Board,
    ) -> Option<Self::Result> {
        Some(event)
    }
}

async fn ask_for_measurement_action(board: &mut Board) -> (bool, bool) {
    let network_configured =
        !board.config.backend_url.is_empty() && !board.config.known_networks.is_empty();
    let can_store = board.storage.is_some();

    if !network_configured && !can_store {
        return (false, false);
    }

    let mut items = heapless::Vec::<_, 3>::new();

    let mut add_item = |label, value| {
        unwrap!(items.push(NavigationItem::new(label, value)).ok());
    };

    if network_configured {
        if can_store {
            add_item("Upload or store", (true, true));
        }
        add_item("Upload", (true, false));
    }

    if can_store {
        add_item("Store", (false, true));
    }

    let menu = create_menu("EKG action")
        .add_items(items)
        .add_item(NavigationItem::new("Discard", (false, false)))
        .build();

    display_menu_screen(menu, board, ReturnEvent::default())
        .await
        .unwrap_or((false, false))
}

async fn try_to_upload(board: &mut Board, buffer: &[u8]) -> StoreMeasurement {
    if board.config.backend_url.is_empty() {
        debug!("No backend URL configured, not uploading.");
        return StoreMeasurement::Store;
    }

    let Some(sta) = board.enable_wifi_sta(StaMode::Enable).await else {
        return StoreMeasurement::Store;
    };

    if !wait_for_connection(&sta, board).await {
        // If we do not have a network connection, save to file.
        return StoreMeasurement::Store;
    }

    // If we found a network, attempt to upload.
    // TODO: only try to upload if we are registered.
    debug!("Trying to upload measurement");

    let mut uploading_msg = heapless::String::<48>::new();
    unwrap!(uwrite!(
        &mut uploading_msg,
        "Uploading measurement: {} bytes",
        buffer.len()
    ));

    display_message(board, uploading_msg.as_str()).await;

    let mut resources = HttpClientResources::new_boxed();

    let client = BufferedTcpClient::new(sta.stack(), &resources.client_state);
    let dns = DnsSocket::new(sta.stack());

    let mut client = HttpClient::new(&client, &dns);
    match upload_measurement(
        &board.config.backend_url,
        &mut client,
        0,
        MeasurementRef { version: 0, buffer },
        &mut resources.rx_buffer,
    )
    .await
    {
        Ok(_) => {
            // Upload successful, do not store in file.
            display_message(board, "Upload successful").await;
            StoreMeasurement::DontStore
        }
        Err(_) => {
            warn!("Failed to upload measurement");
            display_message(board, "Upload failed").await;
            StoreMeasurement::Store
        }
    }
}

async fn upload_stored(board: &mut Board) -> bool {
    let Some(sta) = board.enable_wifi_sta(StaMode::OnDemand).await else {
        return false;
    };

    display_message(board, "Connecting to WiFi").await;

    if !wait_for_connection(&sta, board).await {
        display_message(board, "Failed to connect to WiFi").await;
        return true;
    }

    display_message(board, "Uploading stored measurements...").await;

    let Some(storage) = board.storage.as_mut() else {
        display_message(board, "Storage not available").await;
        return true;
    };

    let Ok(mut dir) = storage.read_dir().await else {
        display_message(board, "Could not read storage").await;
        return true;
    };

    let mut fn_buffer = [0; 64];

    let mut resources = HttpClientResources::new_boxed();

    let client = BufferedTcpClient::new(sta.stack(), &resources.client_state);
    let dns = DnsSocket::new(sta.stack());

    let mut client = HttpClient::new(&client, &dns);

    let mut success = true;
    loop {
        match dir.next(storage).await {
            Ok(file) => {
                let Some(file) = file else {
                    debug!("File is None");
                    break;
                };

                match file.name(storage, &mut fn_buffer).await {
                    Ok(name) if name.starts_with("meas.") => {
                        let Ok(buffer) = load_measurement(file, storage).await else {
                            warn!("Failed to load {}", name);
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
                            success = false;
                            break;
                        }

                        info!("Uploaded {}", name);
                    }
                    Ok(_) | Err(StorageError::InsufficientBuffer) => {
                        // not a measurement file, ignore
                    }
                    Err(e) => {
                        warn!("Failed to read file name: {:?}", e);
                        success = false;
                        break;
                    }
                }
            }
            Err(e) => {
                warn!("Failed to read directory: {:?}", e);
                success = false;
                break;
            }
        }
    }

    let message = if success {
        "Upload successful"
    } else {
        "Failed to upload measurements"
    };
    display_message(board, message).await;

    success
}

struct Measurement {
    version: u32,
    buffer: Box<[u8]>,
}

impl Measurement {
    fn as_ref(&self) -> MeasurementRef<'_> {
        MeasurementRef {
            version: self.version,
            buffer: &self.buffer,
        }
    }
}

struct MeasurementRef<'a> {
    version: u32,
    buffer: &'a [u8],
}

impl RequestBody for MeasurementRef<'_> {
    fn len(&self) -> Option<usize> {
        Some(self.buffer.len() + 4)
    }

    async fn write<W: embedded_io::asynch::Write>(&self, writer: &mut W) -> Result<(), W::Error> {
        writer.write_all(&self.version.to_le_bytes()).await?;
        writer.write_all(self.buffer).await?;

        Ok(())
    }
}

async fn load_measurement<M>(file: DirEntry<M>, storage: &mut Storage<M>) -> Result<Measurement, ()>
where
    M: StorageMedium,
    [(); M::BLOCK_COUNT]:,
{
    let Ok(size) = file.size(storage).await else {
        warn!("Failed to read size");
        return Err(());
    };

    let Ok(mut buffer) = buffer_with_capacity(size, 0) else {
        warn!("Failed to allocate {} bytes", size);
        return Err(());
    };

    let mut reader = file.open().await;
    let version = reader.read_loadable::<u8>(storage).await;
    let version = match version {
        Ok(version) => version,
        Err(e) => {
            warn!("Failed to read data: {:?}", e);
            return Err(());
        }
    };

    if let Err(e) = reader.read_all(storage, buffer.as_mut()).await {
        warn!("Failed to read data: {:?}", e);
        return Err(());
    };

    Ok(Measurement {
        version: version as u32,
        buffer,
    })
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
    samples: MeasurementRef<'_>,
    rx_buffer: &mut [u8],
) -> Result<(), ()>
where
    T: TcpConnect,
    DNS: Dns,
{
    const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
    const UPLOAD_TIMEOUT: Duration = Duration::from_secs(30);

    let mut upload_url = heapless::String::<128>::new();
    if uwrite!(
        &mut upload_url,
        "{}/upload_data/{}",
        url,
        SerialNumber::new()
    )
    .is_err()
    {
        warn!("URL too long");
        return Err(());
    }

    let mut timestamp = heapless::String::<32>::new();
    unwrap!(uwrite!(&mut timestamp, "{}", meas_timestamp));

    debug!("Uploading measurement to {}", upload_url);

    let headers = [("X-Timestamp", timestamp.as_str())];

    let connect = select(
        client.request(Method::POST, &upload_url),
        Timer::after(CONNECT_TIMEOUT),
    )
    .await;

    let mut request = match connect {
        Either::First(Ok(request)) => request
            .headers(&headers) // TODO
            .body(samples),
        Either::First(Err(e)) => {
            warn!("HTTP connect error: {}", e);
            return Err(());
        }
        Either::Second(_) => {
            warn!("Conect timeout");
            return Err(());
        }
    };

    let upload = select(request.send(rx_buffer), Timer::after(UPLOAD_TIMEOUT)).await;

    match upload {
        Either::First(Ok(response)) => {
            if [Status::Ok, Status::Created].contains(&response.status) {
                Ok(())
            } else {
                warn!("HTTP upload failed: {}", response.status);
                for header in response.headers() {
                    if header.0.is_empty() {
                        continue;
                    }
                    debug!(
                        "Header {}: {}",
                        header.0,
                        core::str::from_utf8(header.1).unwrap_or("not a string")
                    );
                }
                Err(())
            }
        }
        Either::First(Err(e)) => {
            warn!("HTTP upload error: {}", e);
            Err(())
        }
        Either::Second(_) => {
            warn!("Timeout");
            Err(())
        }
    }
}

async fn try_store_measurement(board: &mut Board, measurement: &[u8]) -> Result<(), StorageError> {
    debug!("Trying to store measurement");

    let mut saving_msg = heapless::String::<48>::new();
    unwrap!(uwrite!(
        &mut saving_msg,
        "Saving measurement: {} bytes",
        measurement.len()
    ));
    display_message(board, &saving_msg).await;
    let Some(storage) = board.storage.as_mut() else {
        return Ok(());
    };

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

    info!("Measurement saved to {}", filename);

    board.signal_sta_work_available();

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

struct MeasurementWriter<'a>(&'a [u8]);

impl<'a> MeasurementWriter<'a> {
    // We're good with a straight u8 until 127 samples, then we can consider switching to varint.
    const FORMAT_VERSION: u8 = EkgFormat::VERSION;
}

impl FileDataWriter for MeasurementWriter<'_> {
    async fn write<M>(
        &self,
        writer: &mut norfs::writer::Writer<M>,
        storage: &mut Storage<M>,
    ) -> Result<(), StorageError>
    where
        M: StorageMedium,
        [(); M::BLOCK_COUNT]:,
    {
        // Here we only store differences, but not the initial sample. The DC offset does not
        // matter for the analysis, and we can reconstruct everything else from the differences.

        let mut writer = writer.bind(storage);

        writer
            .write_all(&Self::FORMAT_VERSION.to_le_bytes())
            .await?;
        writer.write_all(self.0).await?;

        Ok(())
    }

    fn estimate_length(&self) -> usize {
        Self::FORMAT_VERSION.to_le_bytes().len() + self.0.len()
    }
}
