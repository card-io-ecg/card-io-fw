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
use signal_processing::compressing_buffer::{CompressingBuffer, EkgFormat};

use crate::{
    board::{initialized::Context, storage::StorageError},
    human_readable::BinarySize,
    states::menu::MenuScreen,
    uformat, AppState,
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

    let uploaded = if can_upload {
        cfg_if::cfg_if! {
            if #[cfg(feature = "wifi")] {
                let upload_result = try_to_upload(context, samples).await;
                debug!("Upload result: {:?}", upload_result);
                match upload_result {
                    Ok(()) => true,
                    Err(reason) => {
                        if !can_store {
                            display_discarded(context, reason).await;
                        }
                        false
                    }
                }
            } else {
                false
            }
        }
    } else {
        false
    };

    if can_store && !uploaded {
        let store_result = try_store_measurement(context, samples).await;

        if let Err(e) = store_result {
            context.display_message("Could not store measurement").await;
            error!("Failed to store measurement: {:?}", e);
        }
    }

    // Only upload if we did not store.
    #[cfg(feature = "wifi")]
    if can_upload && uploaded {
        // Drop to free up 90kB of memory.
        core::mem::drop(buffer);

        if context.sta_has_work().await {
            upload_stored(context).await;
        }
    }

    next_state
}

fn backend_ready(context: &Context) -> bool {
    cfg_if::cfg_if! {
        if #[cfg(feature = "wifi")] {
            context.backend_ready().is_ok()
        } else {
            let _ = context;
            false
        }
    }
}

async fn ask_for_measurement_action(context: &mut Context) -> (bool, bool) {
    let backend_ready = backend_ready(context);

    let can_store = context.storage.is_some();

    if !backend_ready && !can_store {
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

    let backend_ready = backend_ready(context);

    let can_store = context.storage.is_some();

    if backend_ready {
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

    storage
        .store_measurement(EkgFormat::VERSION, measurement)
        .await?;

    info!("Measurement saved");

    context.signal_sta_work_available(true);

    Ok(())
}

#[cfg(feature = "wifi")]
mod wifi {
    use super::*;
    use crate::{
        board::{
            initialized::{InnerContext, NotReady, StaMode},
            wifi::sta::StaClient,
        },
        SerialNumber,
    };
    use edge_http::Method;
    use embassy_time::{with_timeout, Duration};
    use network_services::{
        http::{Body, Request},
        url,
    };
    use ufmt::uwrite;

    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    #[cfg_attr(feature = "defmt", derive(defmt::Format))]
    pub enum NotUploaded {
        NotReady(NotReady),
        WifiNotEnabled,
        WifiNotConnected,
        Failed,
    }

    pub async fn display_discarded(context: &mut Context, reason: NotUploaded) {
        const DISCARDED: &str = "Measurement discarded";

        let (reason_message, last_message) = match reason {
            NotUploaded::NotReady(NotReady::NoServerAddress) => {
                (None, "No server address. Measurement discarded")
            }
            NotUploaded::NotReady(NotReady::NoNetwork) => {
                (None, "No saved network. Measurement discarded")
            }
            NotUploaded::NotReady(NotReady::NotPaired) => {
                (None, "Device not paired. Measurement discarded")
            }
            NotUploaded::WifiNotEnabled => (Some("WiFi not enabled"), DISCARDED),
            NotUploaded::WifiNotConnected => (Some("Failed to connect to WiFi"), DISCARDED),
            // `try_to_upload` showed this failure.
            NotUploaded::Failed => (None, DISCARDED),
        };

        if let Some(message) = reason_message {
            context.display_message(message).await;
        }
        context.display_message(last_message).await;
    }

    pub async fn try_to_upload(context: &mut Context, buffer: &[u8]) -> Result<(), NotUploaded> {
        if let Err(reason) = context.backend_ready() {
            debug!("Backend is not ready, not uploading.");
            return Err(NotUploaded::NotReady(reason));
        }

        let sta = if let Some(sta) = context.enable_wifi_sta(StaMode::Enable).await {
            if sta.wait_for_connection(context).await {
                sta
            } else {
                return Err(NotUploaded::WifiNotConnected);
            }
        } else {
            return Err(NotUploaded::WifiNotEnabled);
        };

        // If we found a network, attempt to upload.
        debug!("Trying to upload measurement");

        let Ok(mut client) = sta.client() else {
            context.display_message("Out of memory").await;
            return Err(NotUploaded::Failed);
        };

        match upload_measurement(
            &mut client,
            MeasurementRef { version: 0, buffer },
            &mut context.inner,
        )
        .await
        {
            Ok(_) => {
                context.display_message("Upload successful").await;
                Ok(())
            }
            Err(_) => {
                warn!("Failed to upload measurement");
                context.display_message("Upload failed").await;
                Err(NotUploaded::Failed)
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

        let Ok(mut client) = sta.client() else {
            context.display_message("Out of memory").await;
            return;
        };

        let success = loop {
            let measurement = match storage.load_oldest_measurement().await {
                Ok(Some(measurement)) => measurement,
                Ok(None) => break true,
                Err(e) => {
                    warn!("Failed to load measurement: {:?}", e);
                    break false;
                }
            };

            let samples = MeasurementRef {
                version: u32::from(measurement.version),
                buffer: &measurement.payload,
            };
            if upload_measurement(&mut client, samples, &mut context.inner)
                .await
                .is_err()
            {
                warn!("Failed to upload measurement");
                break false;
            }

            info!("Uploaded measurement");
            if let Err(e) = storage.delete_oldest_measurement().await {
                warn!("Failed to delete measurement: {:?}", e);
                break false;
            }
        };

        let message = if success {
            "Upload successful"
        } else {
            "Failed to upload measurements"
        };
        context.display_message(message).await;

        context.signal_sta_work_available(!success);
    }

    pub struct MeasurementRef<'a> {
        version: u32,
        buffer: &'a [u8],
    }

    pub async fn upload_measurement(
        client: &mut StaClient,
        samples: MeasurementRef<'_>,
        context: &mut InnerContext,
    ) -> Result<(), ()> {
        let uploading_msg = uformat!(
            32,
            "Uploading measurement: {}",
            BinarySize(samples.buffer.len())
        );
        context.display_message(uploading_msg.as_str()).await;

        const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
        const UPLOAD_TIMEOUT: Duration = Duration::from_secs(30);

        let Some(base) = url::parse(context.config.backend_url.as_str()) else {
            warn!("Invalid backend URL");
            return Err(());
        };

        let mut path = heapless::String::<128>::new();
        if uwrite!(&mut path, "{}/upload_data/{}", base.path, SerialNumber).is_err() {
            warn!("URL too long");
            return Err(());
        }

        debug!("Uploading measurement to {}", path);

        let mut connection = match with_timeout(CONNECT_TIMEOUT, client.connect(&base)).await {
            Ok(Ok(connection)) => connection,
            Ok(Err(_)) => return Err(()),
            _ => {
                warn!("Conect timeout");
                return Err(());
            }
        };

        let version = samples.version.to_le_bytes();
        let request = Request {
            method: Method::Post,
            path: &path,
            authorization: None,
            body: Some(Body {
                content_type: "application/octet-stream",
                parts: &[&version, samples.buffer],
            }),
        };

        match with_timeout(UPLOAD_TIMEOUT, connection.send(&request)).await {
            Ok(Ok(response)) if matches!(response.status, 200 | 201) => return Ok(()),
            Ok(Ok(response)) => warn!("HTTP upload failed: {}", response.status),
            Ok(Err(_)) => {}
            _ => warn!("Timeout"),
        }
        Err(())
    }
}
#[cfg(feature = "wifi")]
use wifi::*;
