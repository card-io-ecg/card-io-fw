use core::cell::Cell;

use alloc::boxed::Box;
use edge_http::Method;
use embassy_futures::select::{select, Either};
use embassy_time::{with_timeout, Duration, Instant, Timer};
use embedded_io_async::Read;
use esp_bootloader_esp_idf::partitions::PARTITION_TABLE_MAX_LEN;
use network_services::{http::Request, url};
use ufmt::uwrite;

use crate::{
    board::{
        initialized::{Context, StaMode},
        ota::OtaClient,
        storage::lock_flash,
    },
    human_readable::{BinarySize, Throughput},
    states::menu::AppMenu,
    AppState, SerialNumber,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const READ_TIMEOUT: Duration = Duration::from_secs(10);
const READ_BUFFER_LEN: usize = 4096;

#[derive(Clone, Copy, PartialEq)]
enum UpdateError {
    WifiNotEnabled,
    WifiNotConnected,
    InternalError,
    HttpConnectionFailed,
    HttpConnectionTimeout,
    HttpRequestTimeout,
    HttpRequestFailed,
    WriteError,
    DownloadFailed,
    DownloadTimeout,
    EraseFailed,
    ActivateFailed,
}

#[derive(Clone, Copy, PartialEq)]
enum UpdateResult {
    Success,
    AlreadyUpToDate,
    Failed(UpdateError),
}

pub async fn firmware_update(context: &mut Context) -> AppState {
    let update_result = do_update(context).await;

    let message = match update_result {
        UpdateResult::Success => "Update complete",
        UpdateResult::AlreadyUpToDate => "Already up to date",
        UpdateResult::Failed(e) => match e {
            UpdateError::WifiNotEnabled => "WiFi not enabled",
            UpdateError::WifiNotConnected => "Could not connect to WiFi",
            UpdateError::InternalError => "Update failed: internal error",
            UpdateError::HttpConnectionFailed => "Failed to connect to update server",
            UpdateError::HttpConnectionTimeout => "Connection to update server timed out",
            UpdateError::HttpRequestTimeout => "Update request timed out",
            UpdateError::HttpRequestFailed => "Failed to check for update",
            UpdateError::EraseFailed => "Failed to erase update partition",
            UpdateError::WriteError => "Failed to write update",
            UpdateError::DownloadFailed => "Failed to download update",
            UpdateError::DownloadTimeout => "Download timed out",
            UpdateError::ActivateFailed => "Failed to finalize update",
        },
    };

    context.display_message(message).await;

    if let UpdateResult::Success = update_result {
        AppState::Shutdown
    } else {
        AppState::Menu(AppMenu::Main)
    }
}

async fn do_update(context: &mut Context) -> UpdateResult {
    let sta = if let Some(sta) = context.enable_wifi_sta(StaMode::Enable).await {
        if sta.wait_for_connection(context).await {
            sta
        } else {
            return UpdateResult::Failed(UpdateError::WifiNotConnected);
        }
    } else {
        return UpdateResult::Failed(UpdateError::WifiNotEnabled);
    };

    context.display_message("Looking for updates").await;

    let Ok(mut client) = sta.client() else {
        return UpdateResult::Failed(UpdateError::InternalError);
    };

    let Some(base) = url::parse(context.config.backend_url.as_str()) else {
        error!("Invalid backend URL");
        return UpdateResult::Failed(UpdateError::InternalError);
    };

    let mut path = heapless::String::<128>::new();
    if uwrite!(
        &mut path,
        "{}/firmware/{}/{}/{}",
        base.path,
        env!("HW_VERSION"),
        SerialNumber,
        env!("COMMIT_HASH")
    )
    .is_err()
    {
        error!("URL too long");
        return UpdateResult::Failed(UpdateError::InternalError);
    }

    debug!("Looking for update at {}", path.as_str());

    let mut connection = match with_timeout(CONNECT_TIMEOUT, client.connect(&base)).await {
        Ok(Ok(connection)) => connection,
        Ok(Err(_)) => return UpdateResult::Failed(UpdateError::HttpConnectionFailed),
        Err(_) => return UpdateResult::Failed(UpdateError::HttpConnectionTimeout),
    };

    let request = Request {
        method: Method::Get,
        path: &path,
        authorization: None,
        body: None,
    };
    let mut response = match with_timeout(READ_TIMEOUT, connection.send(&request)).await {
        Ok(Ok(response)) => match response.status {
            200 => response,
            304 => return UpdateResult::AlreadyUpToDate,
            status => {
                warn!("HTTP response error: {}", status);
                return UpdateResult::Failed(UpdateError::HttpRequestFailed);
            }
        },
        Ok(Err(_)) => return UpdateResult::Failed(UpdateError::HttpRequestFailed),
        Err(_) => return UpdateResult::Failed(UpdateError::HttpRequestTimeout),
    };

    let Some(mut flash) = lock_flash().await else {
        warn!("Flash is not available for OTA");
        return UpdateResult::Failed(UpdateError::InternalError);
    };
    let Ok(mut partition_table) = Box::try_new([0u8; PARTITION_TABLE_MAX_LEN]) else {
        warn!("Out of memory while preparing OTA");
        return UpdateResult::Failed(UpdateError::InternalError);
    };
    let Ok(mut buffer) = Box::try_new([0u8; READ_BUFFER_LEN]) else {
        warn!("Out of memory while preparing OTA");
        return UpdateResult::Failed(UpdateError::InternalError);
    };
    let mut ota = match OtaClient::initialize(flash.storage_mut(), &mut partition_table) {
        Ok(ota) => ota,
        Err(e) => {
            warn!("Failed to initialize OTA: {:?}", e);
            return UpdateResult::Failed(UpdateError::InternalError);
        }
    };

    let size = response
        .content_len
        .and_then(|len| usize::try_from(len).ok());
    print_progress(context, 0, size, None).await;

    if let Err(e) = ota.erase().await {
        warn!("Failed to erase OTA: {:?}", e);
        return UpdateResult::Failed(UpdateError::EraseFailed);
    };

    let started = Instant::now();
    let received_since = Cell::new(0);
    let mut received_total = 0;
    let result = select(
        async {
            loop {
                let read = match with_timeout(READ_TIMEOUT, response.body.read(&mut *buffer)).await
                {
                    Ok(Ok(0)) => break None,
                    Ok(Ok(read)) => read,
                    Ok(Err(e)) => {
                        warn!("HTTP read error: {:?}", defmt::Debug2Format(&e));
                        break Some(UpdateError::DownloadFailed);
                    }
                    Err(_) => break Some(UpdateError::DownloadTimeout),
                };

                if let Err(e) = ota.write(&buffer[..read]) {
                    warn!("Failed to write OTA: {:?}", e);
                    break Some(UpdateError::WriteError);
                }

                received_since.set(received_since.get() + read);
            }
        },
        async {
            loop {
                Timer::after(Duration::from_millis(500)).await;
                let received = received_since.take();
                received_total += received;

                let avg_speed = Throughput(received_total, started.elapsed());

                print_progress(context, received_total, size, Some(avg_speed)).await;
            }
        },
    )
    .await;

    match result {
        Either::First(Some(error)) => UpdateResult::Failed(error),
        Either::First(None) => {
            if let Err(e) = ota.activate() {
                warn!("Failed to activate OTA: {:?}", e);
                UpdateResult::Failed(UpdateError::ActivateFailed)
            } else {
                UpdateResult::Success
            }
        }
        Either::Second(_) => unreachable!(),
    }
}

async fn print_progress(
    context: &mut Context,
    current: usize,
    size: Option<usize>,
    speed: Option<Throughput>,
) {
    let mut message = heapless::String::<128>::new();
    if let Some(progress) = size.and_then(|size| (current * 100).checked_div(size)) {
        unwrap!(uwrite!(message, "Downloading update: {}%", progress));
    } else {
        unwrap!(uwrite!(
            message,
            "Downloading update: {}",
            BinarySize(current)
        ));
    }

    if let Some(speed) = speed {
        unwrap!(uwrite!(message, "\n{}", speed));
    }

    context.display_message(message.as_str()).await;
}
