use crate::{
    board::{
        hal::spi::Error as SpiError, initialized::Board, AdcDrdy, AdcReset, AdcSpi, TouchDetect,
    },
    frontend::PoweredFrontend,
    replace_with::replace_with_or_abort_and_return_async,
    states::MIN_FRAME_TIME,
    AppError, AppState,
};
use ads129x::{Error, Sample};
use embassy_executor::{Spawner, _export::StaticCell};
use embassy_sync::{
    blocking_mutex::raw::NoopRawMutex,
    channel::{Channel, Sender},
};
use embassy_time::Ticker;
use embedded_graphics::Drawable;
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

type MessageQueue = Channel<NoopRawMutex, Message, 16>;
type MessageSender = Sender<'static, NoopRawMutex, Message, 16>;

static CHANNEL: StaticCell<MessageQueue> = StaticCell::new();

enum Message {
    Sample(Sample),
    End(EcgFrontend, Result<(), Error<SpiError>>),
}

pub async fn measure(board: &mut Board) -> AppState {
    replace_with_or_abort_and_return_async(board, |mut board| async {
        let frontend = match board.frontend.enable_async().await {
            Ok(frontend) => frontend,
            Err((fe, _err)) => {
                board.frontend = fe;
                return (AppState::Error(AppError::Adc), board);
            }
        };

        let spawner = Spawner::for_current_executor().await;

        let queue = CHANNEL.init(MessageQueue::new());

        spawner.must_spawn(reader_task(queue.sender(), frontend));

        // Downsample by 8 to display around 1 second
        let downsampler = Chain::new(DownSampler::new())
            .append(DownSampler::new())
            .append(DownSampler::new());

        // PLI filtering algo is probably overkill for displaying, but it's fancy
        let mut filter = Chain::new(HIGH_PASS_CUTOFF_1_59HZ)
            .append(
                // this is a huge amount of data to block adaptation, but exact summation gives
                // better result than estimation
                PowerLineFilter::<AdaptationBlocking<Sum<1200>, 50, 20>, 1>::new(1000.0, [50.0]),
            )
            .append(downsampler);

        let mut screen = EcgScreen::new();
        let mut ticker = Ticker::every(MIN_FRAME_TIME);
        loop {
            while let Ok(message) = queue.try_recv() {
                match message {
                    Message::Sample(sample) => {
                        // TODO: store in raw buffer
                        if let Some(downsampled) = filter.update(sample.voltage()) {
                            screen.push(downsampled);
                        }
                    }
                    Message::End(frontend, _result) => {
                        board.frontend = frontend.shut_down();

                        return (AppState::Shutdown, board);
                    }
                }
            }

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
                    return Ok(());
                }

                queue.send(Message::Sample(sample.ch1_sample())).await;
            }
            Err(e) => return Err(e),
        }
    }
}
