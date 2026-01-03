use core::str;

use alloc::boxed::Box;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_menu::{
    builder::MenuBuilder,
    collection::MenuItems,
    interaction::single_touch::SingleTouch,
    items::menu_item::{MenuItem, SelectValue},
    selection_indicator::{style::AnimatedTriangle, AnimatedPosition},
};
use gui::{embedded_layout::object_chain, screens::create_menu};
use norfs::{medium::StorageMedium, writer::FileDataWriter, OnCollision, Storage, StorageError};
use signal_processing::compressing_buffer::{CompressingBuffer, EkgFormat};
use ufmt::uwrite;

use crate::{
    board::initialized::Context, human_readable::BinarySize, states::menu::MenuScreen, uformat,
    AppState,
};
use config_types::types::MeasurementAction;

#[cfg(feature = "wifi")]
pub async fn upload_stored_measurements(context: &mut Context, next_state: AppState) -> AppState {
    upload_stored(context).await;

    next_state
}

pub async fn upload_or_store_measurement<const SIZE: usize>(
    context: &mut Context,
    mut buffer: Box<CompressingBuffer<SIZE>>,
    next_state: AppState,
) -> AppState {
    let sample_count = buffer.len();
    let samples = buffer.make_contiguous();

    const SAMPLE_RATE: usize = 1000; // samples/sec

    debug!("Measurement length: {} samples", sample_count);

    if sample_count < 20 * SAMPLE_RATE {
        if context.config.measurement_action != MeasurementAction::Discard {
            // We don't want to store too-short measurements.
            debug!("Measurement is too short to upload or store.");
            context
                .display_message("Measurement too short, discarding")
                .await;
        }
        return next_state;
    }

    let (can_upload, can_store) = match context.config.measurement_action {
        MeasurementAction::Ask => ask_for_measurement_action(context).await,
        MeasurementAction::Auto => (true, true),
        MeasurementAction::Store => (false, true),
        MeasurementAction::Upload => (true, false),
        MeasurementAction::Discard => (false, false),
    };

    let store_after_upload = if can_upload {
        cfg_if::cfg_if! {
            if #[cfg(feature = "wifi")] {
                let upload_result = try_to_upload(context, samples).await;
                debug!("Upload result: {:?}", upload_result);
                upload_result == StoreMeasurement::Store
            } else {
                true
            }
        }
    } else {
        true
    };

    if can_store && store_after_upload {
        let store_result = try_store_measurement(context, samples).await;

        if let Err(e) = store_result {
            context.display_message("Could not store measurement").await;
            error!("Failed to store measurement: {:?}", e);
        }
    }

    // Only upload if we did not store.
    #[cfg(feature = "wifi")]
    if can_upload && !store_after_upload {
        // Drop to free up 90kB of memory.
        core::mem::drop(buffer);

        if context.sta_has_work().await {
            upload_stored(context).await;
        }
    }

    next_state
}

async fn ask_for_measurement_action(context: &mut Context) -> (bool, bool) {
    let network_configured =
        !context.config.backend_url.is_empty() && !context.config.known_networks.is_empty();

    let can_store = context.storage.is_some();

    if !network_configured && !can_store {
        return (false, false);
    }

    AskForMeasurementActionMenu
        .display(context)
        .await
        .unwrap_or((false, false))
}

struct AskForMeasurementActionMenu;

#[derive(Clone, Copy, PartialEq)]
struct UploadOrStore(bool, bool);
impl SelectValue for UploadOrStore {
    fn marker(&self) -> &'static str {
        ""
    }
}

type AskForMeasurementActionMenuBuilder = MenuBuilder<
    &'static str,
    SingleTouch,
    object_chain::Link<
        MenuItem<&'static str, (bool, bool), UploadOrStore, true>,
        object_chain::Chain<
            MenuItems<
                heapless::Vec<MenuItem<&'static str, (bool, bool), UploadOrStore, true>, 3>,
                MenuItem<&'static str, (bool, bool), UploadOrStore, true>,
                (bool, bool),
            >,
        >,
    >,
    (bool, bool),
    AnimatedPosition,
    AnimatedTriangle,
    BinaryColor,
>;

fn ask_for_action_builder(context: &mut Context) -> AskForMeasurementActionMenuBuilder {
    let mut items = heapless::Vec::<_, 3>::new();

    let mut add_item = |label, can_upload, can_store| {
        unwrap!(items
            .push(
                MenuItem::new(label, UploadOrStore(can_upload, can_store))
                    .with_value_converter(|x| (x.0, x.1))
            )
            .ok());
    };

    let network_configured = cfg!(feature = "wifi")
        && !context.config.backend_url.is_empty()
        && !context.config.known_networks.is_empty();

    let can_store = context.storage.is_some();

    if network_configured {
        if can_store {
            add_item("Upload or store", true, true);
        }
        add_item("Upload", true, false);
    }

    if can_store {
        add_item("Store", false, true);
    }

    create_menu("EKG action").add_menu_items(items).add_item(
        "Discard",
        UploadOrStore(false, false),
        |x| (x.0, x.1),
    )
}

impl MenuScreen for AskForMeasurementActionMenu {
    type Event = (bool, bool);
    type Result = (bool, bool);
    type MenuBuilder = AskForMeasurementActionMenuBuilder;

    async fn menu(&mut self, context: &mut Context) -> Self::MenuBuilder {
        ask_for_action_builder(context)
    }

    async fn handle_event(
        &mut self,
        event: Self::Event,
        _board: &mut Context,
    ) -> Option<Self::Result> {
        Some(event)
    }
}

async fn try_store_measurement(
    context: &mut Context,
    measurement: &[u8],
) -> Result<(), StorageError> {
    debug!("Trying to store measurement");

    let saving_msg = uformat!(32, "Saving measurement: {}", BinarySize(measurement.len()));
    context.display_message(&saving_msg).await;
    let Some(storage) = context.storage.as_mut() else {
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

    context.signal_sta_work_available(true);

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

#[cfg(feature = "wifi")]
mod wifi {
    use super::*;
    use crate::{
        board::initialized::{InnerContext, StaMode},
        SerialNumber,
    };
    use embassy_time::{with_timeout, Duration};
    use embedded_nal_async::{Dns, TcpConnect};
    use norfs::read_dir::DirEntry;
    use reqwless::{
        client::HttpClient,
        request::{Method, RequestBody, RequestBuilder},
        response::Status,
    };

    /// Whether to store the measurement or not. Used instead of a bool to reduce confusion.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    #[cfg_attr(feature = "defmt", derive(defmt::Format))]
    pub enum StoreMeasurement {
        Store,
        DontStore,
    }

    pub async fn try_to_upload(context: &mut Context, buffer: &[u8]) -> StoreMeasurement {
        if context.config.backend_url.is_empty() {
            debug!("No backend URL configured, not uploading.");
            return StoreMeasurement::Store;
        }

        let sta = if let Some(sta) = context.enable_wifi_sta(StaMode::Enable).await {
            if sta.wait_for_connection(context).await {
                sta
            } else {
                // If we do not have a network connection, save to file.
                return StoreMeasurement::Store;
            }
        } else {
            return StoreMeasurement::Store;
        };

        // If we found a network, attempt to upload.
        // TODO: only try to upload if we are registered.
        debug!("Trying to upload measurement");

        let Ok(mut client_resources) = sta.https_client_resources() else {
            context.display_message("Out of memory").await;
            return StoreMeasurement::Store;
        };
        let mut client = client_resources.client();

        match upload_measurement(
            &mut client,
            0,
            MeasurementRef { version: 0, buffer },
            &mut context.inner,
        )
        .await
        {
            Ok(_) => {
                // Upload successful, do not store in file.
                context.display_message("Upload successful").await;
                StoreMeasurement::DontStore
            }
            Err(_) => {
                warn!("Failed to upload measurement");
                context.display_message("Upload failed").await;
                StoreMeasurement::Store
            }
        }
    }

    pub async fn upload_stored(context: &mut Context) {
        let sta = if let Some(sta) = context.enable_wifi_sta(StaMode::OnDemand).await {
            if sta.wait_for_connection(context).await {
                sta
            } else {
                context.display_message("Failed to connect to WiFi").await;
                return;
            }
        } else {
            context.display_message("Nothing to upload").await;
            return;
        };

        context
            .display_message("Uploading stored measurements...")
            .await;

        let Some(storage) = context.storage.as_mut() else {
            context.display_message("Storage not available").await;
            return;
        };

        let Ok(mut dir) = storage.read_dir().await else {
            context.display_message("Could not read storage").await;
            return;
        };

        let mut fn_buffer = [0; 64];

        let Ok(mut client_resources) = sta.https_client_resources() else {
            context.display_message("Out of memory").await;
            return;
        };
        let mut client = client_resources.client();

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
                            let Ok((file, buffer)) = load_measurement(file, storage).await else {
                                warn!("Failed to load {}", name);
                                continue;
                            };

                            if let Err(e) = upload_measurement(
                                &mut client,
                                0,
                                buffer.as_ref(),
                                &mut context.inner,
                            )
                            .await
                            {
                                warn!("Failed to upload {}: {:?}", name, e);
                                success = false;
                                break;
                            }

                            info!("Uploaded {}", name);
                            if let Err(e) = file.delete(storage).await {
                                warn!("Failed to delete file: {:?}", e);
                            }
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
        context.display_message(message).await;

        context.signal_sta_work_available(!success);
    }

    pub struct Measurement {
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

    pub struct MeasurementRef<'a> {
        version: u32,
        buffer: &'a [u8],
    }

    impl RequestBody for MeasurementRef<'_> {
        fn len(&self) -> Option<usize> {
            Some(self.buffer.len() + 4)
        }

        async fn write<W: embedded_io_async::Write>(&self, writer: &mut W) -> Result<(), W::Error> {
            writer.write_all(&self.version.to_le_bytes()).await?;
            writer.write_all(self.buffer).await?;

            Ok(())
        }
    }

    pub async fn load_measurement<M>(
        file: DirEntry<M>,
        storage: &mut Storage<M>,
    ) -> Result<(DirEntry<M>, Measurement), ()>
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

        let mut reader = file.open();
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

        Ok((
            DirEntry::from_reader(reader),
            Measurement {
                version: version as u32,
                buffer,
            },
        ))
    }

    fn buffer_with_capacity<T: Copy>(size: usize, init_val: T) -> Result<Box<[T]>, ()> {
        use core::mem::MaybeUninit;

        let mut buffer = alloc::vec::Vec::new();

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

    pub async fn upload_measurement<T, DNS>(
        client: &mut HttpClient<'_, T, DNS>,
        meas_timestamp: u64,
        samples: MeasurementRef<'_>,
        context: &mut InnerContext,
    ) -> Result<(), ()>
    where
        T: TcpConnect,
        DNS: Dns,
    {
        let uploading_msg = uformat!(
            32,
            "Uploading measurement: {}",
            BinarySize(samples.buffer.len())
        );
        context.display_message(uploading_msg.as_str()).await;

        const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
        const UPLOAD_TIMEOUT: Duration = Duration::from_secs(30);

        let mut upload_url = heapless::String::<128>::new();
        if uwrite!(
            &mut upload_url,
            "{}/upload_data/{}",
            context.config.backend_url.as_str(),
            SerialNumber
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

        let mut request =
            match with_timeout(CONNECT_TIMEOUT, client.request(Method::POST, &upload_url)).await {
                Ok(Ok(request)) => request.headers(&headers).body(samples),
                Ok(Err(e)) => {
                    warn!("HTTP connect error: {:?}", e);
                    return Err(());
                }
                _ => {
                    warn!("Conect timeout");
                    return Err(());
                }
            };

        let mut rx_buffer = [0; 512];
        match with_timeout(UPLOAD_TIMEOUT, request.send(&mut rx_buffer)).await {
            Ok(Ok(response)) => {
                if [Status::Ok, Status::Created].contains(&response.status.into()) {
                    return Ok(());
                }

                warn!("HTTP upload failed: {:?}", response.status);
                for header in response.headers() {
                    if !header.0.is_empty() {
                        debug!(
                            "Header {}: {}",
                            header.0,
                            str::from_utf8(header.1).unwrap_or("not a string")
                        );
                    }
                }
            }
            Ok(Err(e)) => warn!("HTTP upload error: {:?}", e),
            _ => warn!("Timeout"),
        }
        Err(())
    }
}

#[cfg(feature = "wifi")]
use wifi::*;
