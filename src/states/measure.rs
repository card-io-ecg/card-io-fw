use crate::{
    board::{
        config::types::FilterStrength,
        initialized::{Context, InnerContext},
        AdcSpi, EcgFrontend, PoweredEcgFrontend,
    },
    states::{menu::AppMenu, to_progress, INIT_MENU_THRESHOLD, INIT_TIME, MIN_FRAME_TIME},
    task_control::{TaskControlToken, TaskController},
    timeout::Timeout,
    AppState,
};
use ads129x::{Error, Sample};
use alloc::{boxed::Box, sync::Arc};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use embassy_time::{Duration, Instant, Ticker};
use embedded_graphics::Drawable;
use embedded_hal::spi::ErrorType;
use esp_hal::prelude::*;
use gui::screens::{init::StartupScreen, measure::EcgScreen};
use macros as cardio;
use object_chain::{chain, Chain, ChainElement, Link};
use signal_processing::{
    compressing_buffer::CompressingBuffer,
    filter::{
        iir::{precomputed::ALL_PASS, HighPass, Iir, LowPass},
        pli::{adaptation_blocking::AdaptationBlocking, PowerLineFilter},
        Filter,
    },
    heart_rate::HeartRateCalculator,
    moving::sum::EstimatedSum,
};

#[cfg(not(feature = "downsampler-light"))]
use signal_processing::filter::downsample::DownSampler;

type MessageQueue = Channel<CriticalSectionRawMutex, Sample, 32>;

unsafe impl Send for PoweredEcgFrontend {}

struct EcgTaskParams {
    token: TaskControlToken<Result<(), Error<<AdcSpi as ErrorType>::Error>>, PoweredEcgFrontend>,
    sender: Arc<MessageQueue>,
}

// PLI filtering algo is probably overkill for displaying, but it's fancy
pub type EcgFilter = chain! {
    PowerLineFilter<AdaptationBlocking<EstimatedSum<1200>, 4, 19>, Iir<'static, HighPass, 2>, 1>,
    Iir<'static, HighPass, 2>
};

#[cfg(feature = "downsampler-light")]
pub struct DownsamplerLight {
    filter: Iir<'static, LowPass, 2>,
    counter: u8,
}

#[cfg(feature = "downsampler-light")]
impl Filter for DownsamplerLight {
    fn clear(&mut self) {
        self.filter.clear();
        self.counter = 0;
    }

    fn update(&mut self, sample: f32) -> Option<f32> {
        let filtered = self.filter.update(sample)?;
        if self.counter == 0 {
            self.counter = 7;
            Some(filtered)
        } else {
            self.counter -= 1;
            None
        }
    }
}

// Downsample by 8 to display around 1 second
#[cfg(not(feature = "downsampler-light"))]
pub type DownsamplerChain = chain! {
    DownSampler,
    DownSampler,
    DownSampler
};

#[cfg(not(feature = "downsampler-light"))]
type EcgDownsampler = DownsamplerChain;

#[cfg(not(feature = "downsampler-light"))]
fn create_downsampler() -> DownsamplerChain {
    Chain::new(DownSampler::new())
        .append(DownSampler::new())
        .append(DownSampler::new())
}

#[cfg(feature = "downsampler-light")]
type EcgDownsampler = DownsamplerLight;

#[cfg(feature = "downsampler-light")]
fn create_downsampler() -> DownsamplerLight {
    DownsamplerLight {
        #[rustfmt::skip]
        filter: macros::designfilt!(
            "lowpassiir",
            "FilterOrder", 2,
            "HalfPowerFrequency", 35,
            "SampleRate", 1000
        ),
        counter: 7,
    }
}

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
            filter: Chain::new(PowerLineFilter::new_1ksps([50.0])).append(hpf),
            downsampler: create_downsampler(),
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

pub async fn measure(context: &mut Context) -> AppState {
    let filter = match context.config.filter_strength() {
        FilterStrength::None => ALL_PASS,
        #[rustfmt::skip]
        FilterStrength::Weak => macros::designfilt!(
            "highpassiir",
            "FilterOrder", 2,
            "HalfPowerFrequency", 0.75,
            "SampleRate", 1000
        ),
        #[rustfmt::skip]
        FilterStrength::Strong => macros::designfilt!(
            "highpassiir",
            "FilterOrder", 2,
            "HalfPowerFrequency", 1.5,
            "SampleRate", 1000
        ),
    };

    // We allocate two different objects because the filters don't need to outlive this app state.
    let ecg_buffer = Box::try_new(CompressingBuffer::EMPTY).ok();
    let mut ecg = Box::new(EcgObjects::new(filter));

    if ecg_buffer.is_none() {
        warn!("Failed to allocate ECG buffer");
    }

    unsafe {
        let frontend = core::ptr::read(&context.frontend);

        let (next_state, frontend) =
            measure_impl(&mut context.inner, frontend, &mut ecg, ecg_buffer).await;

        core::ptr::write(&mut context.frontend, frontend);
        next_state
    }
}

async fn measure_impl(
    context: &mut InnerContext,
    frontend: EcgFrontend,
    ecg: &mut EcgObjects,
    mut ecg_buffer: Option<Box<CompressingBuffer<ECG_BUFFER_SIZE>>>,
) -> (AppState, EcgFrontend) {
    let mut frontend = match frontend.enable_async().await {
        Ok(frontend) => frontend,
        Err((fe, _err)) => {
            context.display_message("ADC error").await;

            return (AppState::Shutdown, fe);
        }
    };

    match frontend.set_clock_source().await {
        Ok(true) => {
            unwrap!(frontend.spi_mut().bus_mut().apply_config(&{
                let mut config = esp_hal::spi::master::Config::default();
                config.frequency = 4u32.MHz();
                config.mode = esp_hal::spi::SpiMode::Mode1;
                config
            }));
        }

        Err(_e) => {
            context.display_message("ADC error").await;

            return (AppState::Shutdown, frontend.shut_down().await);
        }

        _ => {}
    }

    let queue = Arc::new(MessageQueue::new());

    let task_control = TaskController::from_resources(frontend);

    context
        .high_prio_spawner
        .must_spawn(reader_task(EcgTaskParams {
            token: task_control.token(),
            sender: queue.clone(),
        }));

    ecg.heart_rate_calculator.clear();

    let mut screen = EcgScreen::new();

    let mut samples = 0; // Counter and 1s timer to debug perf issues
    let mut debug_print_timer = Timeout::new(Duration::from_secs(1));

    let mut ticker = Ticker::every(MIN_FRAME_TIME);
    let mut drop_samples = 1500; // Slight delay for the input to settle
    let mut entered = Instant::now();
    let exit_timer = Timeout::new_with_start(INIT_TIME, entered - INIT_MENU_THRESHOLD);

    while !task_control.has_exited() && !context.battery_monitor.is_low() {
        let display_full = screen.buffer_full();
        while let Ok(sample) = queue.try_receive() {
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
                        screen.push(downsampled);
                    }
                }
            } else {
                drop_samples -= 1;
            }
        }

        if !display_full {
            if screen.buffer_full() {
                entered = Instant::now();
            }
            if let Some(ecg_buffer) = ecg_buffer.as_deref_mut() {
                ecg_buffer.clear();
            }
        }

        if debug_print_timer.is_elapsed() {
            debug!(
                "Collected {} samples in {}ms",
                samples,
                debug_print_timer.elapsed().as_millis()
            );
            samples = 0;
            debug_print_timer.reset();
        }

        context
            .with_status_bar(|display| {
                if !exit_timer.is_elapsed() {
                    StartupScreen {
                        label: "Release to menu",
                        progress: to_progress(exit_timer.elapsed(), INIT_TIME),
                    }
                    .draw(display)
                } else {
                    screen.update_heart_rate(ecg.heart_rate_calculator.current_hr());
                    screen.elapsed_secs = entered.elapsed().as_secs() as usize;

                    screen.draw(display)
                }
            })
            .await;

        ticker.next().await;
    }

    let result = task_control.stop().await;
    let next_state = match result {
        Ok(result) => {
            // task stopped itself
            if let Err(_e) = result.as_ref() {
                warn!("Measurement task error"); // TODO: print error once supported
            }
            if result.is_ok() && !exit_timer.is_elapsed() {
                AppState::Menu(AppMenu::Main)
            } else if let Some(ecg_buffer) = ecg_buffer {
                AppState::UploadOrStore(ecg_buffer)
            } else {
                AppState::Shutdown
            }
        }
        Err(_) => {
            // task was aborted - battery low
            AppState::Shutdown
        }
    };

    let frontend = task_control.unwrap();

    (next_state, frontend.shut_down().await)
}

#[cardio::task]
async fn reader_task(params: EcgTaskParams) {
    let EcgTaskParams { mut token, sender } = params;

    token
        .run_cancellable(|frontend| read_ecg(sender.as_ref(), frontend))
        .await;
    info!("Measurement task stopped");
}

async fn read_ecg(
    queue: &MessageQueue,
    frontend: &mut PoweredEcgFrontend,
) -> Result<(), Error<<AdcSpi as ErrorType>::Error>> {
    loop {
        match frontend.read().await {
            Ok(sample) => {
                if !frontend.is_touched() {
                    info!("Not touched, stopping");
                    return Ok(());
                }

                if queue.try_send(sample.ch1_sample()).is_err() {
                    warn!("Sample lost");
                }
            }
            Err(e) => return Err(e),
        }
    }
}
