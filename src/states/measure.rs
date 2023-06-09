use crate::{
    board::{
        hal::{prelude::*, spi::Error as SpiError},
        initialized::Board,
        PoweredEcgFrontend, LOW_BATTERY_VOLTAGE,
    },
    replace_with::replace_with_or_abort_and_return_async,
    states::{BIG_OBJECTS, MIN_FRAME_TIME},
    AppError, AppState,
};
use ads129x::{descriptors::PinState, Error, Sample};
use embassy_executor::_export::StaticCell;
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    channel::{Channel, Sender},
    signal::Signal,
};
use embassy_time::{Duration, Instant, Ticker, Timer};
use embedded_graphics::Drawable;
use gui::screens::measure::EcgScreen;
use object_chain::{chain, Chain, Link};
use signal_processing::{
    filter::{
        downsample::DownSampler,
        iir::{HighPass, Iir},
        pli::{adaptation_blocking::AdaptationBlocking, PowerLineFilter},
        Filter,
    },
    moving::sum::Sum,
};

type MessageQueue = Channel<CriticalSectionRawMutex, Message, 32>;
type MessageSender = Sender<'static, CriticalSectionRawMutex, Message, 32>;

static CHANNEL: StaticCell<MessageQueue> = StaticCell::new();
static THREAD_CONTROL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

enum Message {
    Sample(Sample),
    End(PoweredEcgFrontend, Result<(), Error<SpiError>>),
}

unsafe impl Send for Message {} // SAFETY: yolo

struct EcgTaskParams {
    frontend: PoweredEcgFrontend,
    sender: MessageSender,
}

unsafe impl Send for EcgTaskParams {} // SAFETY: yolo

// PLI filtering algo is probably overkill for displaying, but it's fancy
// this is a huge amount of data to block adaptation, but exact summation gives
// better result than estimation (TODO: revisit later, as estimated sum had a bug)
pub type EcgFilter = chain! {
    Iir<'static, HighPass, 2>,
    PowerLineFilter<AdaptationBlocking<Sum<1200>, 50, 20>, 1>
};

// Downsample by 8 to display around 1 second
pub type EcgDownsampler = chain! {
    DownSampler,
    DownSampler,
    DownSampler
};

pub async fn measure(board: &mut Board) -> AppState {
    let ecg = unsafe { BIG_OBJECTS.as_ecg() };

    ecg.downsampler.clear();
    ecg.filter.clear();
    ecg.buffer.clear();

    replace_with_or_abort_and_return_async(board, |mut board| async {
        let mut frontend = match board.frontend.enable_async().await {
            Ok(frontend) => frontend,
            Err((fe, _err)) => {
                board.frontend = fe;
                return (AppState::Error(AppError::Adc), board);
            }
        };

        let ret = match frontend.read_clksel().await {
            Ok(PinState::Low) => {
                log::info!("CLKSEL low, enabling faster clock speeds");
                let result = frontend.enable_fast_clock().await;

                if result.is_ok() {
                    frontend
                        .spi_mut()
                        .spi
                        .change_bus_frequency(4u32.MHz(), &board.clocks);
                }

                result
            }

            Ok(PinState::High) => Ok(()),
            Err(e) => Err(e),
        };

        if ret.is_err() {
            board.frontend = frontend.shut_down().await;
            return (AppState::Error(AppError::Adc), board);
        }

        let queue = CHANNEL.init(MessageQueue::new());

        board
            .high_prio_spawner
            .must_spawn(reader_task(EcgTaskParams {
                sender: queue.sender(),
                frontend,
            }));

        ecg.heart_rate_calculator.clear();

        let mut screen = EcgScreen::new(96); // discard transient
        screen.battery_style = board.config.battery_style();

        let mut ticker = Ticker::every(MIN_FRAME_TIME);

        let mut samples = 0; // Counter and 1s timer to debug perf issues
        let mut started = Instant::now();
        loop {
            while let Ok(message) = queue.try_recv() {
                match message {
                    Message::Sample(sample) => {
                        samples += 1;
                        ecg.buffer.push(sample.raw());
                        if let Some(filtered) = ecg.filter.update(sample.voltage()) {
                            ecg.heart_rate_calculator.update(filtered);
                            if let Some(downsampled) = ecg.downsampler.update(filtered) {
                                screen.push(downsampled);
                            }
                        }
                    }
                    Message::End(frontend, _result) => {
                        board.frontend = frontend.shut_down().await;

                        return (AppState::Shutdown, board);
                    }
                }
            }

            if let Some(hr) = ecg.heart_rate_calculator.current_hr() {
                screen.update_heart_rate(hr);
            } else {
                screen.clear_heart_rate();
            }

            let now = Instant::now();
            if now - started > Duration::from_secs(1) {
                log::debug!(
                    "Collected {} samples in {}ms",
                    samples,
                    (now - started).as_millis()
                );
                samples = 0;
                started = now;
            }

            let battery_data = board.battery_monitor.battery_data().await;

            if let Some(battery) = battery_data {
                if battery.voltage < LOW_BATTERY_VOLTAGE {
                    THREAD_CONTROL.signal(());
                }
            }
            screen.battery_data = battery_data;
            board
                .display
                .frame(|display| screen.draw(display))
                .await
                .unwrap();

            ticker.next().await;
        }
    })
    .await
}

#[embassy_executor::task]
async fn reader_task(params: EcgTaskParams) {
    let EcgTaskParams {
        sender,
        mut frontend,
    } = params;

    Timer::after(Duration::from_millis(100)).await;

    let result = read_ecg(&sender, &mut frontend).await;
    sender.send(Message::End(frontend, result)).await;
}

async fn read_ecg(
    queue: &MessageSender,
    frontend: &mut PoweredEcgFrontend,
) -> Result<(), Error<SpiError>> {
    while !THREAD_CONTROL.signaled() {
        match frontend.read().await {
            Ok(sample) => {
                if !frontend.is_touched() {
                    log::info!("Not touched, stopping");
                    return Ok(());
                }

                if queue
                    .try_send(Message::Sample(sample.ch1_sample()))
                    .is_err()
                {
                    log::warn!("Sample lost");
                }
            }
            Err(e) => return Err(e),
        }
    }

    log::info!("Stop requested, stopping");
    Ok(())
}
