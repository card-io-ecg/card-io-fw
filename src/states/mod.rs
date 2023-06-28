mod adc_setup;
mod charging;
mod error;
mod init;
mod measure;
mod menu;
mod wifi_ap;

use crate::states::measure::{EcgDownsampler, EcgFilter};
use embassy_net::StackResources;
use embassy_time::Duration;
use object_chain::{Chain, ChainElement};
use signal_processing::filter::{
    downsample::DownSampler, iir::precomputed::HIGH_PASS_CUTOFF_1_59HZ, pli::PowerLineFilter,
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

pub struct EcgObjects {
    pub filter: EcgFilter,
    pub downsampler: EcgDownsampler,
}

impl EcgObjects {
    fn new() -> Self {
        Self {
            filter: Chain::new(HIGH_PASS_CUTOFF_1_59HZ)
                .append(PowerLineFilter::new(1000.0, [50.0])),
            downsampler: Chain::new(DownSampler::new())
                .append(DownSampler::new())
                .append(DownSampler::new()),
        }
    }
}

pub struct WifiApResources {
    resources: [WebserverResources; WEBSERVER_TASKS],
    stack_resources: StackResources<3>,
}

impl WifiApResources {
    fn new() -> Self {
        Self {
            resources: [WebserverResources::ZERO; 2],
            stack_resources: StackResources::new(),
        }
    }
}

#[allow(clippy::large_enum_variant)]
pub enum BigObjects {
    Unused,
    WifiAp(WifiApResources),
    Ecg(EcgObjects),
}

impl BigObjects {
    #[inline(never)]
    pub fn as_wifi_ap_resources(&mut self) -> &mut WifiApResources {
        if !matches!(self, Self::WifiAp { .. }) {
            unsafe { core::ptr::write(self, Self::WifiAp(WifiApResources::new())) }
        }

        match self {
            Self::WifiAp(resources) => resources,
            _ => unreachable!(),
        }
    }

    #[inline(never)]
    pub fn as_ecg(&mut self) -> &mut EcgObjects {
        if !matches!(self, Self::Ecg { .. }) {
            unsafe { core::ptr::write(self, Self::Ecg(EcgObjects::new())) }
        }

        match self {
            Self::Ecg(ecg) => ecg,
            _ => unreachable!(),
        }
    }
}

pub static mut BIG_OBJECTS: BigObjects = BigObjects::Unused;
