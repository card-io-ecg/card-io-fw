mod adc_setup;
mod charging;
mod error;
mod init;
mod measure;
mod menu;
mod wifi_ap;

use core::mem::MaybeUninit;

use crate::states::measure::{EcgDownsampler, EcgFilter};
use embassy_net::StackResources;
use embassy_time::Duration;
use object_chain::{Chain, ChainElement};
use signal_processing::{
    buffer::Buffer,
    filter::{
        downsample::DownSampler, iir::precomputed::HIGH_PASS_CUTOFF_1_59HZ, pli::PowerLineFilter,
    },
    heart_rate::HeartRateCalculator,
    i24::i24,
};

pub use adc_setup::adc_setup;
pub use charging::charging;
pub use error::app_error;
pub use init::initialize;
pub use measure::measure;
pub use menu::{display::display_menu, main::main_menu};
pub use wifi_ap::wifi_ap;

const TARGET_FPS: u32 = 100;
const MIN_FRAME_TIME: Duration = Duration::from_hz(TARGET_FPS as u64);

// The max number of webserver tasks.
const WEBSERVER_TASKS: usize = 2;

#[derive(Clone, Copy)]
pub struct WebserverResources {
    tx_buffer: [u8; 4096],
    rx_buffer: [u8; 4096],
    request_buffer: [u8; 2048],
}

impl WebserverResources {
    const ZERO: Self = Self {
        tx_buffer: [0; 4096],
        rx_buffer: [0; 4096],
        request_buffer: [0; 2048],
    };
}

const ECG_BUFFER_SIZE: usize = 30_000;

pub struct EcgObjects {
    pub filter: EcgFilter,
    pub downsampler: EcgDownsampler,
    pub heart_rate_calculator: HeartRateCalculator,
    pub buffer: Buffer<i24, ECG_BUFFER_SIZE>,
}

impl EcgObjects {
    #[inline(always)]
    fn init(this: &mut MaybeUninit<Self>) {
        let this = this.as_mut_ptr();

        unsafe {
            (*this).filter =
                Chain::new(HIGH_PASS_CUTOFF_1_59HZ).append(PowerLineFilter::new(1000.0, [50.0]));
            (*this).downsampler = Chain::new(DownSampler::new())
                .append(DownSampler::new())
                .append(DownSampler::new());
            (*this).heart_rate_calculator = HeartRateCalculator::new(1000.0);
            (*this).buffer = Buffer::EMPTY;
        }
    }
}

pub struct WifiApResources {
    resources: [WebserverResources; WEBSERVER_TASKS],
    stack_resources: StackResources<3>,
}

impl WifiApResources {
    #[inline(always)]
    fn init(this: &mut MaybeUninit<Self>) {
        let this = this.as_mut_ptr();

        unsafe {
            (*this).resources = [WebserverResources::ZERO; WEBSERVER_TASKS];
            (*this).stack_resources = StackResources::new();
        }
    }
}

#[allow(clippy::large_enum_variant)]
pub enum BigObjects {
    Unused,
    WifiAp(MaybeUninit<WifiApResources>),
    Ecg(MaybeUninit<EcgObjects>),
}

impl BigObjects {
    #[inline(always)]
    fn initialize(&mut self) {
        match self {
            Self::Ecg(ecg) => EcgObjects::init(ecg),
            Self::WifiAp(ap) => WifiApResources::init(ap),
            _ => unreachable!(),
        }
    }

    #[inline(never)]
    pub fn as_wifi_ap_resources(&mut self) -> &mut WifiApResources {
        if !matches!(self, Self::WifiAp { .. }) {
            *self = Self::WifiAp(MaybeUninit::uninit());
            self.initialize();
        }

        let Self::WifiAp(ap) = self else {
            unreachable!()
        };

        unsafe { ap.assume_init_mut() }
    }

    #[inline(never)]
    pub fn as_ecg(&mut self) -> &mut EcgObjects {
        if !matches!(self, Self::Ecg { .. }) {
            *self = Self::Ecg(MaybeUninit::uninit());
            self.initialize();
        }

        let Self::Ecg(ecg) = self else { unreachable!() };

        unsafe { ecg.assume_init_mut() }
    }
}

pub static mut BIG_OBJECTS: BigObjects = BigObjects::Unused;
