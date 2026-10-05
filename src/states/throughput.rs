use core::cell::Cell;

use alloc::boxed::Box;
use edge_http::Method;
use embassy_futures::select::{select, Either};
use embassy_time::{with_timeout, Duration, Instant, Timer};
use embedded_io_async::Read;
use network_services::{
    http::{Request, Response},
    pairing::{Signer, Step, Template},
    url,
};
use ufmt::{uwrite, uwriteln};

use crate::{
    board::initialized::{Context, StaMode},
    human_readable::{BinarySize, Throughput},
    states::menu::AppMenu,
    AppState,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const READ_TIMEOUT: Duration = Duration::from_secs(10);
const READ_BUFFER_LEN: usize = 4096;

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
    Refused,
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
            TestError::Refused => "Server refused this device",
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

    let Some(signing) = context.signing() else {
        return TestResult::Failed(TestError::InternalError);
    };
    let Ok(mut client) = sta.client() else {
        return TestResult::Failed(TestError::InternalError);
    };
    let Ok(mut buffer) = Box::try_new([0u8; READ_BUFFER_LEN]) else {
        warn!("Out of memory while preparing the test");
        return TestResult::Failed(TestError::InternalError);
    };

    context.display_message("Connecting to server...").await;

    let Some(base) = url::parse(context.config.backend_url.as_str()) else {
        error!("Invalid backend URL");
        return TestResult::Failed(TestError::InternalError);
    };

    let mut path = heapless::String::<128>::new();
    if uwrite!(
        &mut path,
        "{}/firmware/{}/0000000",
        base.path,
        env!("HW_VERSION")
    )
    .is_err()
    {
        error!("URL too long");
        return TestResult::Failed(TestError::InternalError);
    }

    debug!("Testing throughput using {}", path.as_str());

    let mut counters = signing.counters.lock().await;
    let mut signer = Signer::new(
        &signing.key,
        &signing.name,
        &mut counters,
        Template::Firmware,
        Method::Get,
        &path,
    );
    loop {
        let authorization = signer.authorization();
        let mut connection = match with_timeout(CONNECT_TIMEOUT, client.connect(&base)).await {
            Ok(Ok(connection)) => connection,
            Ok(Err(_)) => return TestResult::Failed(TestError::HttpConnectionFailed),
            Err(_) => return TestResult::Failed(TestError::HttpConnectionTimeout),
        };

        let request = Request {
            method: Method::Get,
            path: &path,
            authorization: Some(&authorization),
            body: None,
        };
        let mut response = match with_timeout(READ_TIMEOUT, connection.send(&request)).await {
            Ok(Ok(response)) => response,
            Ok(Err(_)) => return TestResult::Failed(TestError::HttpRequestFailed),
            Err(_) => return TestResult::Failed(TestError::HttpRequestTimeout),
        };

        match signer.answered(response.status, response.counter) {
            Step::Resend => {}
            Step::Refused => {
                context.pairing.refused();
                return TestResult::Failed(TestError::Refused);
            }
            Step::Answered(200) => return measure(context, &mut response, &mut *buffer).await,
            Step::Answered(status) => {
                warn!("HTTP response error: {}", status);
                return TestResult::Failed(TestError::HttpRequestFailed);
            }
        }
    }
}

async fn measure<R: Read>(
    context: &mut Context,
    response: &mut Response<R>,
    buffer: &mut [u8],
) -> TestResult {
    let size = response
        .content_len
        .and_then(|len| usize::try_from(len).ok());
    let mut received_total = 0;
    let started = Instant::now();
    let received_since = Cell::new(0);
    let result = select(
        async {
            loop {
                match with_timeout(READ_TIMEOUT, response.body.read(buffer)).await {
                    Ok(Ok(0)) => break None,
                    Ok(Ok(read)) => received_since.set(received_since.get() + read),
                    Ok(Err(e)) => {
                        warn!("HTTP read error: {:?}", defmt::Debug2Format(&e));
                        break Some(TestError::DownloadFailed);
                    }
                    Err(_) => break Some(TestError::DownloadTimeout),
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
    if let Some(progress) = size.and_then(|size| (current * 100).checked_div(size)) {
        unwrap!(uwriteln!(message, "Testing: {}%", progress));
    } else {
        unwrap!(uwriteln!(message, "Testing: {}", BinarySize(current)));
    }
    unwrap!(uwriteln!(message, "Current: {}", current_tp));
    unwrap!(uwrite!(message, "Average: {}", average_tp));

    context.display_message(message.as_str()).await;
}
