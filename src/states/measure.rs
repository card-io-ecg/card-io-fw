use core::ops::{Deref, DerefMut};

use crate::{
    board::{
        hal::{prelude::*, spi::Error as SpiError},
        initialized::Board,
        EcgFrontend, PoweredEcgFrontend,
    },
    replace_with::replace_with_or_abort_and_return_async,
    states::{
        display_message_while_touched, menu::AppMenu, to_progress, INIT_MENU_THRESHOLD, INIT_TIME,
        MIN_FRAME_TIME,
    },
    timeout::Timeout,
    AppState,
};
use ads129x::{Error, Sample};
use alloc::{boxed::Box, sync::Arc};
use embassy_futures::select::{select, Either};
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, signal::Signal,
};
use embassy_time::{Duration, Instant, Ticker};
use embedded_graphics::Drawable;
use embedded_hal_bus::spi::DeviceError;
use gui::{
    screens::{
        display_menu::FilterStrength, init::StartupScreen, measure::EcgScreen, screen::Screen,
    },
    widgets::{battery_small::Battery, status_bar::StatusBar, wifi::WifiStateView},
};
use macros as cardio;
use object_chain::{chain, Chain, ChainElement, Link};
use signal_processing::{
    compressing_buffer::CompressingBuffer,
    filter::{
        downsample::DownSampler,
        iir::{
            precomputed::{
                HIGH_PASS_FOR_DISPLAY_NONE, HIGH_PASS_FOR_DISPLAY_STRONG,
                HIGH_PASS_FOR_DISPLAY_WEAK,
            },
            HighPass, Iir, LowPass,
        },
        pli::{adaptation_blocking::AdaptationBlocking, PowerLineFilter},
        Filter,
    },
    heart_rate::HeartRateCalculator,
    moving::sum::EstimatedSum,
};

type MessageQueue = Channel<CriticalSectionRawMutex, Message, 32>;

static THREAD_CONTROL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

struct SharedFrontend(Box<PoweredEcgFrontend>);
unsafe impl Send for SharedFrontend {} // SAFETY: yolo
impl SharedFrontend {
    pub fn new(frontend: PoweredEcgFrontend) -> Self {
        Self(Box::new(frontend))
    }

    pub async fn shut_down(self) -> EcgFrontend {
        self.0.shut_down().await
    }
}
impl Deref for SharedFrontend {
    type Target = PoweredEcgFrontend;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl DerefMut for SharedFrontend {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

enum Message {
    Sample(Sample),
    End(SharedFrontend, Result<(), Error<SpiError>>),
}

struct EcgTaskParams {
    frontend: SharedFrontend,
    sender: Arc<MessageQueue>,
}

// PLI filtering algo is probably overkill for displaying, but it's fancy
pub type EcgFilter = chain! {
    PowerLineFilter<AdaptationBlocking<EstimatedSum<1200>, 4, 19>, Iir<'static, HighPass, 2>, 1>,
    Iir<'static, HighPass, 2>
};

// Downsample by 8 to display around 1 second
pub type EcgDownsampler = chain! {
    DownSampler,
    DownSampler,
    DownSampler
};

pub const ECG_BUFFER_SIZE: usize = 90_000;

// Two filter chains:
// - PLI -> IIR HPF -> FIR Downsample -> display
// - PLI -> IIR HPF -> FIR LPF in HR calculator -> HR calculator
struct EcgObjects {
    pub filter: EcgFilter,
    pub downsampler: EcgDownsampler,
    pub heart_rate_calculator: HeartRateCalculator<[f32; 300], [f32; 50]>,
    pub hr_noise_filter: Iir<'static, LowPass, 2>,
}

impl EcgObjects {
    #[inline(always)]
    fn new(hpf: Iir<'static, HighPass, 2>) -> Self {
        Self {
            filter: Chain::new(PowerLineFilter::new(1000.0, [50.0])).append(hpf),
            downsampler: Chain::new(DownSampler::new())
                .append(DownSampler::new())
                .append(DownSampler::new()),
            heart_rate_calculator: HeartRateCalculator::new(1000.0),

            #[rustfmt::skip]
            hr_noise_filter: macros::designfilt!(
                "lowpassiir",
                "FilterOrder", 2,
                "HalfPowerFrequency", 20,
                "SampleRate", 1000
            ),
        }
    }
}

pub async fn measure(board: &mut Board) -> AppState {
    let filter = match board.config.filter_strength() {
        FilterStrength::None => HIGH_PASS_FOR_DISPLAY_NONE,
        FilterStrength::Weak => HIGH_PASS_FOR_DISPLAY_WEAK,
        FilterStrength::Strong => HIGH_PASS_FOR_DISPLAY_STRONG,
    };
    let ecg_buffer = Box::try_new(CompressingBuffer::EMPTY).ok();
    let mut ecg = Box::new(EcgObjects::new(filter));

    if ecg_buffer.is_none() {
        warn!("Failed to allocate ECG buffer");
    }

    replace_with_or_abort_and_return_async(board, |board| async {
        measure_impl(board, &mut ecg, ecg_buffer).await
    })
    .await
}

async fn measure_impl(
    mut board: Board,
    ecg: &mut EcgObjects,
    mut ecg_buffer: Option<Box<CompressingBuffer<ECG_BUFFER_SIZE>>>,
) -> (AppState, Board) {
    let mut frontend = match board.frontend.enable_async().await {
        Ok(frontend) => SharedFrontend::new(frontend),
        Err((fe, _err)) => {
            board.frontend = fe;

            display_message_while_touched(&mut board, "ADC error").await;

            return (AppState::Shutdown, board);
        }
    };

    match frontend.set_clock_source().await {
        Ok(true) => {
            frontend
                .spi_mut()
                .bus_mut()
                .change_bus_frequency(4u32.MHz(), &board.clocks);
        }

        Err(_e) => {
            board.frontend = frontend.shut_down().await;
            display_message_while_touched(&mut board, "ADC error").await;

            return (AppState::Shutdown, board);
        }

        _ => {}
    }

    let queue = Arc::new(MessageQueue::new());

    board
        .high_prio_spawner
        .must_spawn(reader_task(EcgTaskParams {
            sender: queue.clone(),
            frontend,
        }));

    ecg.heart_rate_calculator.clear();

    let mut screen = Screen {
        content: EcgScreen::new(),

        status_bar: StatusBar {
            battery: Battery::with_style(
                board.battery_monitor.battery_data(),
                board.config.battery_style(),
            ),
            wifi: WifiStateView::disabled(),
        },
    };

    let mut samples = 0; // Counter and 1s timer to debug perf issues
    let mut debug_print_timer = Timeout::new(Duration::from_secs(1));

    let mut ticker = Ticker::every(MIN_FRAME_TIME);
    let exit_timer = Timeout::new_with_start(INIT_TIME, Instant::now() - INIT_MENU_THRESHOLD);
    let mut drop_samples = 1500; // Slight delay for the input to settle
    let mut entered = Instant::now();
    loop {
        let display_full = screen.content.buffer_full();
        while let Ok(message) = queue.try_recv() {
            match message {
                Message::Sample(sample) => {
                    samples += 1;

                    if drop_samples == 0 {
                        if let Some(ecg_buffer) = ecg_buffer.as_deref_mut() {
                            ecg_buffer.push(sample.raw());
                        }
                        if let Some(filtered) = ecg.filter.update(sample.voltage()) {
                            if let Some(filtered) = ecg.hr_noise_filter.update(filtered) {
                                ecg.heart_rate_calculator.update(filtered);
                            }

                            if let Some(downsampled) = ecg.downsampler.update(filtered) {
                                screen.content.push(downsampled);
                            }
                        }
                    } else {
                        drop_samples -= 1;
                    }
                }
                Message::End(frontend, result) => {
                    board.frontend = frontend.shut_down().await;

                    return if result.is_ok() && !exit_timer.is_elapsed() {
                        (AppState::Menu(AppMenu::Main), board)
                    } else if let Some(ecg_buffer) = ecg_buffer {
                        (AppState::UploadOrStore(ecg_buffer), board)
                    } else {
                        (AppState::Shutdown, board)
                    };
                }
            }
        }

        if !display_full {
            if screen.content.buffer_full() {
                entered = Instant::now();
            }
            if let Some(ecg_buffer) = ecg_buffer.as_deref_mut() {
                ecg_buffer.clear();
            }
        }

        if let Some(hr) = ecg.heart_rate_calculator.current_hr() {
            screen.content.update_heart_rate(hr);
        } else {
            screen.content.clear_heart_rate();
        }
        screen.content.elapsed_secs = entered.elapsed().as_secs() as usize;

        if debug_print_timer.is_elapsed() {
            debug!(
                "Collected {} samples in {}ms",
                samples,
                debug_print_timer.elapsed().as_millis()
            );
            samples = 0;
            debug_print_timer.reset();
        }

        if board.battery_monitor.is_low() {
            THREAD_CONTROL.signal(());
        }

        let battery_data = board.battery_monitor.battery_data();
        let status_bar = StatusBar {
            battery: Battery::with_style(battery_data, board.config.battery_style()),
            wifi: WifiStateView::disabled(),
        };

        if !exit_timer.is_elapsed() {
            let init_screen = Screen {
                content: StartupScreen {
                    label: "Release to menu",
                    progress: to_progress(exit_timer.elapsed(), INIT_TIME),
                },

                status_bar,
            };

            board
                .display
                .frame(|display| init_screen.draw(display))
                .await;
        } else {
            screen.status_bar = status_bar;

            board.display.frame(|display| screen.draw(display)).await;
        }

        ticker.next().await;
    }
}

#[cardio::task]
async fn reader_task(params: EcgTaskParams) {
    let EcgTaskParams {
        sender,
        mut frontend,
    } = params;

    let result = select(
        read_ecg(sender.as_ref(), &mut frontend),
        THREAD_CONTROL.wait(),
    )
    .await;

    let result = match result {
        Either::First(result) => result,
        Either::Second(_) => Ok(()),
    };

    info!("Stopping measurement task");
    sender.send(Message::End(frontend, result)).await;
}

async fn read_ecg(
    queue: &MessageQueue,
    frontend: &mut PoweredEcgFrontend,
) -> Result<(), Error<SpiError>> {
    loop {
        match frontend.read().await {
            Ok(sample) => {
                if !frontend.is_touched() {
                    info!("Not touched, stopping");
                    return Ok(());
                }

                if queue
                    .try_send(Message::Sample(sample.ch1_sample()))
                    .is_err()
                {
                    warn!("Sample lost");
                }
            }
            Err(e) => {
                return Err(match e {
                    Error::InvalidState => Error::InvalidState,
                    Error::UnexpectedDeviceId => Error::UnexpectedDeviceId,
                    Error::Verification => Error::Verification,
                    Error::Transfer(DeviceError::Spi(e)) => Error::Transfer(e),
                    Error::Transfer(DeviceError::Cs(_)) => unreachable!(),
                });
            }
        }
    }
}
