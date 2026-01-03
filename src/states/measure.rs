use crate::{
    board::{
        initialized::{Context, InnerContext},
        AdcSpi, EcgFrontend, PoweredEcgFrontend,
    },
    states::{menu::AppMenu, to_progress, INIT_MENU_THRESHOLD, INIT_TIME, MIN_FRAME_TIME},
    task_control::{TaskControlToken, TaskController},
    timeout::Timeout,
    AppState,
};
use ads129x::{ll, ConfigRegisters, Sample};
use alloc::{boxed::Box, sync::Arc};
use config_types::types::{
    FilterStrength, Gain, LeadOffCurrent, LeadOffFrequency, LeadOffThreshold,
};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use embassy_time::{Duration, Instant, Ticker};
use embedded_graphics::Drawable;
use embedded_hal::spi::ErrorType;
use esp_hal::time::Rate;
use gui::screens::{init::StartupScreen, measure::EcgScreen};
use macros as cardio;
use object_chain::{chain, Chain, ChainElement, Link};
use signal_processing::{
    compressing_buffer::CompressingBuffer,
    filter::{
        iir::{
            precomputed::{ALL_PASS, HR_NOISE_FILTER, STRONG_EKG_1000HZ, WEAK_EKG_1000HZ},
            HighPass, Iir, LowPass,
        },
        pli::{adaptation_blocking::AdaptationBlocking, PowerLineFilter},
        Filter,
    },
    heart_rate::HeartRateCalculator,
    moving::sum::EstimatedSum,
};

type MessageQueue = Channel<CriticalSectionRawMutex, Sample, 32>;

unsafe impl Send for PoweredEcgFrontend {}

struct EcgTaskParams {
    token: TaskControlToken<Result<(), <AdcSpi as ErrorType>::Error>, PoweredEcgFrontend>,
    sender: Arc<MessageQueue>,
}

// PLI filtering algo is probably overkill for displaying, but it's fancy
pub type EcgFilter = chain! {
    PowerLineFilter<AdaptationBlocking<EstimatedSum<1200>, 4, 19>, Iir<'static, HighPass, 2>, 1>,
    Iir<'static, HighPass, 2>
};

cfg_if::cfg_if! {
    if #[cfg(feature = "downsampler-light")] {
        use signal_processing::filter::downsample_light::DownsamplerLight;

        type EcgDownsampler = DownsamplerLight;

        fn create_downsampler() -> EcgDownsampler {
            DownsamplerLight::ECG_SR_1000HZ
        }
    } else {
        use signal_processing::filter::downsample::DownSampler;

        // Downsample by 8 to display around 1 second
        type EcgDownsampler = chain! {
            DownSampler,
            DownSampler,
            DownSampler
        };

        fn create_downsampler() -> EcgDownsampler {
            Chain::new(DownSampler::new())
                .append(DownSampler::new())
                .append(DownSampler::new())
        }
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
            hr_noise_filter: HR_NOISE_FILTER,
        }
    }
}

pub async fn measure(context: &mut Context) -> AppState {
    let filter = match context.config.filter_strength() {
        FilterStrength::None => ALL_PASS,
        FilterStrength::Weak => WEAK_EKG_1000HZ,
        FilterStrength::Strong => STRONG_EKG_1000HZ,
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

        // Reset SPI bus configuration
        unwrap!(context.frontend.spi_mut().bus_mut().apply_config(
            &esp_hal::spi::master::Config::default()
                .with_frequency(Rate::from_mhz(1))
                .with_mode(esp_hal::spi::Mode::_1)
        ));

        next_state
    }
}

async fn measure_impl(
    context: &mut InnerContext,
    frontend: EcgFrontend,
    ecg: &mut EcgObjects,
    mut ecg_buffer: Option<Box<CompressingBuffer<ECG_BUFFER_SIZE>>>,
) -> (AppState, EcgFrontend) {
    let apply_config = |config_regs: &mut ConfigRegisters| {
        let loff_current_value = match context.config.lead_off_current {
            LeadOffCurrent::Weak => ll::LeadOffCurrent::_6nA,
            LeadOffCurrent::Normal => ll::LeadOffCurrent::_22nA,
            LeadOffCurrent::Strong => ll::LeadOffCurrent::_6uA,
            LeadOffCurrent::Strongest => ll::LeadOffCurrent::_22uA,
        };
        let loff_threshold_value = match context.config.lead_off_threshold {
            LeadOffThreshold::_95 => ll::ComparatorThreshold::_95,
            LeadOffThreshold::_92_5 => ll::ComparatorThreshold::_92_5,
            LeadOffThreshold::_90 => ll::ComparatorThreshold::_90,
            LeadOffThreshold::_87_5 => ll::ComparatorThreshold::_87_5,
            LeadOffThreshold::_85 => ll::ComparatorThreshold::_85,
            LeadOffThreshold::_80 => ll::ComparatorThreshold::_80,
            LeadOffThreshold::_75 => ll::ComparatorThreshold::_75,
            LeadOffThreshold::_70 => ll::ComparatorThreshold::_70,
        };
        let loff_frequency_value = match context.config.lead_off_frequency {
            LeadOffFrequency::Dc => ll::LeadOffFrequency::Dc,
            LeadOffFrequency::Ac => ll::LeadOffFrequency::Ac,
        };
        let gain_value = match context.config.gain {
            Gain::X1 => ll::Gain::X1,
            Gain::X2 => ll::Gain::X2,
            Gain::X3 => ll::Gain::X3,
            Gain::X4 => ll::Gain::X4,
            Gain::X6 => ll::Gain::X6,
            Gain::X8 => ll::Gain::X8,
            Gain::X12 => ll::Gain::X12,
        };

        config_regs.loff.set_leadoff_current(loff_current_value);
        config_regs.loff.set_comp_th(loff_threshold_value);
        config_regs.loff.set_leadoff_frequency(loff_frequency_value);
        config_regs.ch1set.set_gain(gain_value);
    };
    let mut frontend = match frontend.enable_async(apply_config).await {
        Ok(frontend) => frontend,
        Err((fe, err)) => {
            let err_str = match err {
                ads129x::AdsConfigError::ReadbackMismatch => "Failed to start ADC: config error",
                ads129x::AdsConfigError::Spi(_) => "Failed to start ADC: SPI error",
            };
            context.display_message(err_str).await;

            // Enter main menu in case of an init error, so that the device is not entirely unusable.
            return (AppState::Menu(AppMenu::Main), fe);
        }
    };

    match frontend
        .set_clock_source(context.config.use_external_clock)
        .await
    {
        Ok(true) => {
            unwrap!(frontend.spi_mut().bus_mut().apply_config(
                &esp_hal::spi::master::Config::default()
                    .with_frequency(Rate::from_mhz(4))
                    .with_mode(esp_hal::spi::Mode::_1)
            ));
        }

        Err(_e) => {
            context.display_message("ADC error").await;

            return (AppState::Shutdown, frontend.shut_down().await);
        }

        _ => {}
    }

    if let Err(e) = frontend.start().await {
        error!("ADC start error: {}", e);
        context.display_message("ADC start error").await;

        return (AppState::Shutdown, frontend.shut_down().await);
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
) -> Result<(), <AdcSpi as ErrorType>::Error> {
    loop {
        let sample = frontend.read().await?;

        if !frontend.is_touched() {
            info!("Not touched, stopping");
            return Ok(());
        }

        if queue.try_send(sample.ch1_sample()).is_err() {
            warn!("Sample lost");
        }
    }
}
