use core::cell::Cell;

use embassy_futures::select::{select, Either};
use embassy_time::{Duration, Instant, Timer};
use embedded_io_async::BufRead;
use reqwless::{request::Method, response::Status};
use ufmt::uwrite;

use crate::{
    board::{
        initialized::{Context, StaMode},
        ota::{Ota0Partition, Ota1Partition, OtaClient, OtaDataPartition},
    },
    human_readable::{BinarySize, Throughput},
    states::menu::AppMenu,
    timeout::Timeout,
    AppState, SerialNumber,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const READ_TIMEOUT: Duration = Duration::from_secs(10);

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

    let Ok(mut client_resources) = sta.https_client_resources() else {
        return UpdateResult::Failed(UpdateError::InternalError);
    };
    let mut client = client_resources.client();

    let mut url = heapless::String::<128>::new();
    if uwrite!(
        &mut url,
        "{}/firmware/{}/{}/{}",
        context.config.backend_url.as_str(),
        env!("HW_VERSION"),
        SerialNumber,
        env!("COMMIT_HASH")
    )
    .is_err()
    {
        error!("URL too long");
        return UpdateResult::Failed(UpdateError::InternalError);
    }

    debug!("Looking for update at {}", url.as_str());

    let mut request = match Timeout::with(CONNECT_TIMEOUT, client.request(Method::GET, &url)).await
    {
        Some(Ok(request)) => request,
        Some(Err(e)) => {
            warn!("HTTP connect error: {:?}", e);
            return UpdateResult::Failed(UpdateError::HttpConnectionFailed);
        }
        None => return UpdateResult::Failed(UpdateError::HttpConnectionTimeout),
    };

    let mut rx_buffer = [0; 4096];
    let result = match Timeout::with(READ_TIMEOUT, request.send(&mut rx_buffer)).await {
        Some(result) => result,
        _ => return UpdateResult::Failed(UpdateError::HttpRequestTimeout),
    };

    let response = match result {
        Ok(response) => match response.status {
            Status::Ok => response,
            Status::NotModified => return UpdateResult::AlreadyUpToDate,
            _ => {
                warn!("HTTP response error: {:?}", response.status);
                return UpdateResult::Failed(UpdateError::HttpRequestFailed);
            }
        },
        Err(e) => {
            warn!("HTTP response error: {:?}", e);
            return UpdateResult::Failed(UpdateError::HttpRequestFailed);
        }
    };

    let mut ota = match OtaClient::initialize(OtaDataPartition, Ota0Partition, Ota1Partition).await
    {
        Ok(ota) => ota,
        Err(e) => {
            warn!("Failed to initialize OTA: {:?}", e);
            return UpdateResult::Failed(UpdateError::InternalError);
        }
    };

    let size = response.content_length;
    print_progress(context, 0, size, None).await;

    if let Err(e) = ota.erase().await {
        warn!("Failed to erase OTA: {:?}", e);
        return UpdateResult::Failed(UpdateError::EraseFailed);
    };

    let mut reader = response.body().reader();

    let started = Instant::now();
    let received_since = Cell::new(0);
    let mut received_total = 0;
    let result = select(
        async {
            loop {
                let received_buffer = match Timeout::with(READ_TIMEOUT, reader.fill_buf()).await {
                    Some(result) => match result {
                        Ok(&[]) => break None,
                        Ok(read) => read,
                        Err(e) => {
                            warn!("HTTP read error: {:?}", e);
                            break Some(UpdateError::DownloadFailed);
                        }
                    },
                    _ => break Some(UpdateError::DownloadTimeout),
                };

                if let Err(e) = ota.write(received_buffer).await {
                    warn!("Failed to write OTA: {:?}", e);
                    break Some(UpdateError::WriteError);
                }

                let received_len = received_buffer.len();
                received_since.set(received_since.get() + received_len);
                reader.consume(received_len);
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
            if let Err(e) = ota.activate().await {
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
    if let Some(size) = size {
        let progress = current * 100 / size;
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
