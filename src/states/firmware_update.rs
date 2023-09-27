use embassy_futures::select::{select, Either};
use embassy_net::{dns::DnsSocket, tcp::client::TcpClient};
use embassy_time::{Duration, Instant, Timer};
use embedded_io::asynch::Read;
use reqwless::{client::HttpClient, request::Method, response::Status};
use ufmt::uwrite;

use crate::{
    board::{
        initialized::{Board, StaMode},
        ota::{Ota0Partition, Ota1Partition, OtaClient, OtaDataPartition},
        wait_for_connection, HttpClientResources,
    },
    states::{display_message, menu::AppMenu},
    AppState, SerialNumber,
};

pub async fn firmware_update(board: &mut Board) -> AppState {
    if !do_update(board).await {
        AppState::Menu(AppMenu::Main)
    } else {
        AppState::Shutdown
    }
}

async fn do_update(board: &mut Board) -> bool {
    display_message(board, "Connecting to WiFi").await;

    let Some(sta) = board.enable_wifi_sta(StaMode::Enable).await else {
        display_message(board, "Could not enable WiFi").await;
        return false;
    };

    if !wait_for_connection(&sta, board).await {
        return false;
    }

    display_message(board, "Looking for updates").await;

    let mut resources = HttpClientResources::new_boxed();

    let client = TcpClient::new(sta.stack(), &resources.client_state);
    let dns = DnsSocket::new(sta.stack());

    let mut client = HttpClient::new(&client, &dns);

    const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
    const READ_TIMEOUT: Duration = Duration::from_secs(10);

    let mut url = heapless::String::<128>::new();
    if uwrite!(
        &mut url,
        "{}/firmware/{}/{}/{}",
        board.config.backend_url.as_str(),
        env!("HW_VERSION"),
        SerialNumber::new(),
        env!("COMMIT_HASH")
    )
    .is_err()
    {
        error!("URL too long");
        return false;
    }

    debug!("Looking for update at {}", url.as_str());

    let connect = select(
        client.request(Method::GET, &url),
        Timer::after(CONNECT_TIMEOUT),
    )
    .await;

    let mut request = match connect {
        Either::First(Ok(request)) => request,
        Either::First(Err(e)) => {
            display_message(board, "Connection failed").await;
            warn!("HTTP connect error: {}", e);
            return false;
        }
        Either::Second(_) => {
            display_message(board, "Connection timeout").await;
            warn!("Conect timeout");
            return false;
        }
    };

    let Either::First(result) = select(
        request.send(&mut resources.rx_buffer),
        Timer::after(READ_TIMEOUT),
    )
    .await
    else {
        display_message(board, "Update request timed out").await;
        return false;
    };

    let response = match result {
        Ok(response) => match response.status {
            Status::Ok => response,
            Status::NoContent => {
                display_message(board, "Already up to date").await;
                return false;
            }
            _ => {
                display_message(board, "Failed to download update").await;
                warn!("HTTP response error: {}", response.status);
                return false;
            }
        },
        Err(e) => {
            display_message(board, "Failed to download update").await;
            warn!("HTTP response error: {}", e);
            return false;
        }
    };

    let mut ota = match OtaClient::initialize(OtaDataPartition, Ota0Partition, Ota1Partition).await
    {
        Ok(ota) => ota,
        Err(e) => {
            display_message(board, "Failed to initialize OTA client").await;
            warn!("Failed to initialize OTA: {}", e);
            return false;
        }
    };

    let size = response.content_length;
    let mut current = 0;

    let mut message_buffer = heapless::String::<128>::new();
    print_progress(board, &mut message_buffer, current, size).await;

    if let Err(e) = ota.erase().await {
        display_message(board, "Failed to erase update partition").await;
        warn!("Failed to erase OTA: {}", e);
        return false;
    };

    let mut reader = response.body().reader();

    let mut buffer = [0; 1024];

    let mut started = Instant::now();
    let mut received_1s = 0;
    loop {
        let Either::First(result) =
            select(reader.read(&mut buffer), Timer::after(READ_TIMEOUT)).await
        else {
            display_message(board, "Downloading update timed out").await;
            warn!("HTTP read timeout");
            return false;
        };

        let read = match result {
            Ok(0) => break,
            Ok(read) => read,
            Err(e) => {
                display_message(board, "Failed to download update").await;
                warn!("HTTP read error: {}", e);
                return false;
            }
        };

        current += read;
        received_1s += read;

        let elapsed_ms = started.elapsed().as_millis();
        if elapsed_ms > 500 {
            let kib_per_sec = received_1s as f32 / elapsed_ms as f32;

            debug!(
                "got {}B in {}ms {} KB/s",
                received_1s, elapsed_ms, kib_per_sec
            );
            started = Instant::now();
            received_1s = 0;

            print_progress(board, &mut message_buffer, current, size).await;
        }

        if let Err(e) = ota.write(&buffer[..read]).await {
            display_message(board, "Failed to write update").await;
            warn!("Failed to write OTA: {}", e);
            return false;
        }
    }

    if let Err(e) = ota.activate().await {
        display_message(board, "Failed to activate update").await;
        warn!("Failed to activate OTA: {}", e);
        return false;
    }

    display_message(board, "Update complete").await;
    true
}

async fn print_progress(
    board: &mut Board,
    message: &mut heapless::String<128>,
    current: usize,
    size: Option<usize>,
) {
    message.clear();
    if let Some(size) = size {
        let progress = current * 100 / size;
        unwrap!(uwrite!(message, "Downloading update: {}%", progress));
    } else {
        unwrap!(uwrite!(message, "Downloading update: {} bytes", current));
    }
    display_message(board, message.as_str()).await;
}
