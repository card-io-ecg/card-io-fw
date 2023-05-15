use crate::{
    board::{
        hal::{prelude::*, spi::Error as SpiError},
        initialized::Board,
        AdcDrdy, AdcReset, AdcSpi, TouchDetect,
    },
    frontend::PoweredFrontend,
    replace_with::replace_with_or_abort_and_return_async,
    states::MIN_FRAME_TIME,
    AppError, AppState,
};
use ads129x::{descriptors::PinState, Error, Sample};
use embassy_executor::{Spawner, _export::StaticCell};
use embassy_sync::{
    blocking_mutex::raw::NoopRawMutex,
    channel::{Channel, Sender},
};
use embassy_time::{Duration, Instant, Ticker};
use embedded_graphics::{pixelcolor::BinaryColor, prelude::DrawTarget};
use gui::screens::measure::EcgScreen;
use object_chain::{Chain, ChainElement};
use signal_processing::{
    filter::{
        downsample::DownSampler,
        iir::precomputed::HIGH_PASS_CUTOFF_1_59HZ,
        pli::{adaptation_blocking::AdaptationBlocking, PowerLineFilter},
        Filter,
    },
    moving::sum::Sum,
};

type EcgFrontend = PoweredFrontend<AdcSpi<'static>, AdcDrdy, AdcReset, TouchDetect>;

type MessageQueue = Channel<NoopRawMutex, Message, 32>;
type MessageSender = Sender<'static, NoopRawMutex, Message, 32>;

static CHANNEL: StaticCell<MessageQueue> = StaticCell::new();

enum Message {
    Sample(Sample),
    End(EcgFrontend, Result<(), Error<SpiError>>),
}

pub async fn measure(board: &mut Board) -> AppState {
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

        let spawner = Spawner::for_current_executor().await;

        let queue = CHANNEL.init(MessageQueue::new());

        spawner.must_spawn(reader_task(queue.sender(), frontend));

        // Downsample by 16 to display around 2 seconds
        let downsampler = Chain::new(DownSampler::new())
            .append(DownSampler::new())
            .append(DownSampler::new())
            .append(DownSampler::new());

        // PLI filtering algo is probably overkill for displaying, but it's fancy
        // this is a huge amount of data to block adaptation, but exact summation gives
        // better result than estimation (TODO: revisit later, as estimated sum had a bug)
        let mut filter = Chain::new(HIGH_PASS_CUTOFF_1_59HZ)
            // FIXME: Disabled while we can't reliably sample the ADC
            //.append(
            //    PowerLineFilter::<AdaptationBlocking<Sum<1200>, 50, 20>, 1>::new(1000.0, [50.0]),
            //)
            .append(downsampler);

        let mut screen = EcgScreen::new(96); // discard transient
        let mut ticker = Ticker::every(MIN_FRAME_TIME);

        let mut samples = 0; // Counter and 1s timer to debug perf issues
        let mut started = Instant::now();
        loop {
            while let Ok(message) = queue.try_recv() {
                match message {
                    Message::Sample(sample) => {
                        samples += 1;
                        // TODO: store in raw buffer
                        if let Some(downsampled) = filter.update(sample.voltage()) {
                            screen.push(downsampled);
                        }
                    }
                    Message::End(frontend, _result) => {
                        board.frontend = frontend.shut_down().await;

                        return (AppState::Shutdown, board);
                    }
                }
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

            // Yield after filtering as it may take some time
            embassy_futures::yield_now().await;
            board.display.clear(BinaryColor::Off).unwrap();
            embassy_futures::yield_now().await;
            screen.draw_async(&mut board.display).await.unwrap();
            board.display.flush().await.unwrap();

            ticker.next().await;
        }
    })
    .await
}

#[embassy_executor::task]
async fn reader_task(queue: MessageSender, mut frontend: EcgFrontend) {
    let result = read_ecg(&queue, &mut frontend).await;
    queue.send(Message::End(frontend, result)).await;
}

async fn read_ecg(
    queue: &MessageSender,
    frontend: &mut EcgFrontend,
) -> Result<(), Error<SpiError>> {
    loop {
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
}
