use embassy_futures::select::{select, Either};
use embassy_time::{Duration, Instant, Timer};
use embedded_io::asynch::Read;
use reqwless::{request::Method, response::Status};
use ufmt::{uwrite, uwriteln};

use crate::{
    board::{
        initialized::{Board, StaMode},
        wait_for_connection,
    },
    human_readable::{BinarySize, Throughput},
    states::{display_message, menu::AppMenu},
    AppState, SerialNumber,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const READ_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, PartialEq)]
enum TestError {
    WifiNotEnabled,
    WifiNotConnected,
    InternalError,
    HttpConnectionFailed,
    HttpConnectionTimeout,
    HttpRequestTimeout,
    HttpRequestFailed,
    DownloadFailed,
    DownloadTimeout,
}

#[derive(Clone, Copy, PartialEq)]
enum TestResult {
    Success(Throughput),
    Failed(TestError),
}

pub async fn throughput(board: &mut Board) -> AppState {
    let update_result = run_test(board).await;

    let mut message = heapless::String::<64>::new();
    let message = match update_result {
        TestResult::Success(speed) => {
            unwrap!(uwrite!(
                &mut message,
                "Test complete. Average speed: {} KiB/s",
                speed
            ));
            &message
        }
        TestResult::Failed(e) => match e {
            TestError::WifiNotEnabled => "WiFi not enabled",
            TestError::WifiNotConnected => "Could not connect to WiFi",
            TestError::InternalError => "Test failed: internal error",
            TestError::HttpConnectionFailed => "Failed to connect to server",
            TestError::HttpConnectionTimeout => "Connection to server timed out",
            TestError::HttpRequestTimeout => "Test request timed out",
            TestError::HttpRequestFailed => "Failed to access test data",
            TestError::DownloadFailed => "Failed to download test data",
            TestError::DownloadTimeout => "Test timed out",
        },
    };

    display_message(board, message).await;

    AppState::Menu(AppMenu::Main)
}

async fn run_test(board: &mut Board) -> TestResult {
    display_message(board, "Connecting to WiFi").await;

    let Some(sta) = board.enable_wifi_sta(StaMode::Enable).await else {
        return TestResult::Failed(TestError::WifiNotEnabled);
    };

    display_message(board, "Connecting...").await;

    if !wait_for_connection(&sta, board).await {
        return TestResult::Failed(TestError::WifiNotConnected);
    }

    let Ok(mut client_resources) = sta.https_client_resources() else {
        return TestResult::Failed(TestError::InternalError);
    };
    let mut client = client_resources.client();

    let mut url = heapless::String::<128>::new();
    if uwrite!(
        &mut url,
        "{}/firmware/{}/{}/0000000",
        board.config.backend_url.as_str(),
        env!("HW_VERSION"),
        SerialNumber::new()
    )
    .is_err()
    {
        error!("URL too long");
        return TestResult::Failed(TestError::InternalError);
    }

    debug!("Testing throughput using {}", url.as_str());

    let connect = select(
        client.request(Method::GET, &url),
        Timer::after(CONNECT_TIMEOUT),
    )
    .await;

    let mut request = match connect {
        Either::First(Ok(request)) => request,
        Either::First(Err(e)) => {
            warn!("HTTP connect error: {}", e);
            return TestResult::Failed(TestError::HttpConnectionFailed);
        }
        Either::Second(_) => return TestResult::Failed(TestError::HttpConnectionTimeout),
    };

    let mut rx_buffer = [0; 512];
    let result = match select(request.send(&mut rx_buffer), Timer::after(READ_TIMEOUT)).await {
        Either::First(result) => result,
        _ => return TestResult::Failed(TestError::HttpRequestTimeout),
    };

    let response = match result {
        Ok(response) => match response.status {
            Status::Ok => response,
            _ => {
                warn!("HTTP response error: {}", response.status);
                return TestResult::Failed(TestError::HttpRequestFailed);
            }
        },
        Err(e) => {
            warn!("HTTP response error: {}", e);
            return TestResult::Failed(TestError::HttpRequestFailed);
        }
    };

    let size = response.content_length;
    let mut received_total = 0;
    let mut buffer = [0; 512];

    let mut reader = response.body().reader();

    let started = Instant::now();
    let mut last_print = Instant::now();
    let mut received_1s = 0;
    loop {
        let received_len = match select(reader.read(&mut buffer), Timer::after(READ_TIMEOUT)).await
        {
            Either::First(result) => match result {
                Ok(0) => break,
                Ok(read) => read,
                Err(e) => {
                    warn!("HTTP read error: {}", e);
                    return TestResult::Failed(TestError::DownloadFailed);
                }
            },
            _ => return TestResult::Failed(TestError::DownloadTimeout),
        };

        received_1s += received_len;

        let elapsed = last_print.elapsed();
        if elapsed.as_millis() > 500 {
            received_total += received_1s;

            let speed = Throughput(received_1s, elapsed);
            let avg_speed = Throughput(received_total, started.elapsed());

            received_1s = 0;
            last_print = Instant::now();

            print_progress(board, &mut buffer, received_total, size, speed, avg_speed).await;
        }
    }

    TestResult::Success(Throughput(received_total, started.elapsed()))
}

async fn print_progress(
    board: &mut Board,
    message: &mut [u8],
    current: usize,
    size: Option<usize>,
    current_tp: Throughput,
    average_tp: Throughput,
) {
    let mut message = slice_string::SliceString::new(message);
    if let Some(size) = size {
        let progress = current * 100 / size;
        unwrap!(uwriteln!(message, "Testing: {}%", progress));
    } else {
        unwrap!(uwriteln!(message, "Testing: {}", BinarySize(current)));
    }
    unwrap!(uwriteln!(message, "Current: {}", current_tp));
    unwrap!(uwrite!(message, "Average: {}", average_tp));

    display_message(board, message.as_str()).await;
}
