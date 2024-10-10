use core::cell::Cell;

use embassy_futures::select::{select, Either};
use embassy_time::{with_timeout, Duration, Instant, Timer};
use embedded_io_async::BufRead;
use reqwless::{request::Method, response::Status};
use ufmt::{uwrite, uwriteln};

use crate::{
    board::initialized::{Context, StaMode},
    human_readable::{BinarySize, Throughput},
    states::menu::AppMenu,
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

pub async fn throughput(context: &mut Context) -> AppState {
    let update_result = run_test(context).await;

    let mut message = heapless::String::<64>::new();
    let message = match update_result {
        TestResult::Success(speed) => {
            unwrap!(uwrite!(
                &mut message,
                "Test complete. Average speed: {}",
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

    context.display_message(message).await;

    AppState::Menu(AppMenu::Main)
}

async fn run_test(context: &mut Context) -> TestResult {
    let sta = if let Some(sta) = context.enable_wifi_sta(StaMode::Enable).await {
        if sta.wait_for_connection(context).await {
            sta
        } else {
            return TestResult::Failed(TestError::WifiNotConnected);
        }
    } else {
        return TestResult::Failed(TestError::WifiNotEnabled);
    };

    let Ok(mut client_resources) = sta.https_client_resources() else {
        return TestResult::Failed(TestError::InternalError);
    };
    let mut client = client_resources.client();

    let mut url = heapless::String::<128>::new();
    if uwrite!(
        &mut url,
        "{}/firmware/{}/{}/0000000",
        context.config.backend_url.as_str(),
        env!("HW_VERSION"),
        SerialNumber
    )
    .is_err()
    {
        error!("URL too long");
        return TestResult::Failed(TestError::InternalError);
    }

    debug!("Testing throughput using {}", url.as_str());

    let connect = with_timeout(CONNECT_TIMEOUT, async {
        let futures = select(client.request(Method::GET, &url), async {
            loop {
                // A message is displayed for at least 300ms so we don't need to wait here.
                context.display_message("Connecting to server...").await;
            }
        });
        match futures.await {
            Either::First(request) => request,
            Either::Second(_) => unreachable!(),
        }
    });

    let mut request = match connect.await {
        Ok(Ok(request)) => request,
        Ok(Err(e)) => {
            warn!("HTTP connect error: {:?}", e);
            return TestResult::Failed(TestError::HttpConnectionFailed);
        }
        _ => return TestResult::Failed(TestError::HttpConnectionTimeout),
    };

    let mut rx_buffer = [0; 4096];
    let result = match with_timeout(READ_TIMEOUT, request.send(&mut rx_buffer)).await {
        Ok(result) => result,
        _ => return TestResult::Failed(TestError::HttpRequestTimeout),
    };

    let response = match result {
        Ok(response) => match response.status.into() {
            Status::Ok => response,
            _ => {
                warn!("HTTP response error: {:?}", response.status);
                return TestResult::Failed(TestError::HttpRequestFailed);
            }
        },
        Err(e) => {
            warn!("HTTP response error: {:?}", e);
            return TestResult::Failed(TestError::HttpRequestFailed);
        }
    };

    for header in response.headers() {
        if !header.0.is_empty() {
            debug!(
                "Header {}: {}",
                header.0,
                core::str::from_utf8(header.1).unwrap_or("not a string")
            );
        }
    }

    let size = response.content_length;
    let mut received_total = 0;

    let mut reader = response.body().reader();

    let started = Instant::now();
    let received_since = Cell::new(0);
    let result = select(
        async {
            loop {
                match with_timeout(READ_TIMEOUT, reader.fill_buf()).await {
                    Ok(result) => match result {
                        Ok(&[]) => break None,
                        Ok(read) => {
                            let read_len = read.len();
                            received_since.set(received_since.get() + read_len);
                            reader.consume(read_len);
                        }
                        Err(e) => {
                            warn!("HTTP read error: {:?}", e);
                            break Some(TestError::DownloadFailed);
                        }
                    },
                    _ => break Some(TestError::DownloadTimeout),
                };
            }
        },
        async {
            let mut last_print = Instant::now();
            loop {
                Timer::after(Duration::from_millis(500)).await;
                let received = received_since.take();
                received_total += received;

                let speed = Throughput(received, last_print.elapsed());
                let avg_speed = Throughput(received_total, started.elapsed());

                last_print = Instant::now();

                print_progress(context, received_total, size, speed, avg_speed).await;
            }
        },
    )
    .await;

    match result {
        Either::First(Some(error)) => TestResult::Failed(error),
        Either::First(None) => TestResult::Success(Throughput(
            received_total + received_since.get(),
            started.elapsed(),
        )),
        Either::Second(_) => unreachable!(),
    }
}

async fn print_progress(
    context: &mut Context,
    current: usize,
    size: Option<usize>,
    current_tp: Throughput,
    average_tp: Throughput,
) {
    let mut message = heapless::String::<128>::new();
    if let Some(size) = size {
        let progress = current * 100 / size;
        unwrap!(uwriteln!(message, "Testing: {}%", progress));
    } else {
        unwrap!(uwriteln!(message, "Testing: {}", BinarySize(current)));
    }
    unwrap!(uwriteln!(message, "Current: {}", current_tp));
    unwrap!(uwrite!(message, "Average: {}", average_tp));

    context.display_message(message.as_str()).await;
}
