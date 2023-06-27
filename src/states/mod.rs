mod adc_setup;
mod charging;
mod error;
mod init;
mod measure;
mod menu;
mod wifi_ap;

use embassy_net::StackResources;
use embassy_time::Duration;

pub use adc_setup::adc_setup;
pub use charging::charging;
pub use error::app_error;
pub use init::initialize;
pub use measure::measure;
pub use menu::{display::display_menu, main::main_menu};
use object_chain::{Chain, ChainElement};
use signal_processing::{
    filter::{
        downsample::DownSampler,
        iir::precomputed::HIGH_PASS_CUTOFF_1_59HZ,
        pli::{adaptation_blocking::AdaptationBlocking, PowerLineFilter},
    },
    moving::sum::Sum,
};
pub use wifi_ap::wifi_ap;

use crate::states::measure::{EcgDownsampler, EcgFilter};

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

#[allow(clippy::large_enum_variant)]
pub enum BigObjects {
    Unused,
    WifiApResources {
        resources: [WebserverResources; WEBSERVER_TASKS],
        stack_resources: StackResources<3>,
    },
    Ecg(EcgObjects),
}

impl BigObjects {
    pub fn as_wifi_ap_resources(
        &mut self,
    ) -> (
        &mut [WebserverResources; WEBSERVER_TASKS],
        &mut StackResources<3>,
    ) {
        if !matches!(self, Self::WifiApResources { .. }) {
            *self = Self::WifiApResources {
                resources: [WebserverResources::ZERO; 2],
                stack_resources: StackResources::new(),
            }
        }

        match self {
            Self::WifiApResources {
                resources,
                stack_resources,
            } => (resources, stack_resources),
            _ => unreachable!(),
        }
    }

    pub fn as_ecg(&mut self) -> &mut EcgObjects {
        if !matches!(self, Self::Ecg { .. }) {
            *self = Self::Ecg(EcgObjects {
                filter: Chain::new(HIGH_PASS_CUTOFF_1_59HZ).append(PowerLineFilter::<
                    AdaptationBlocking<Sum<1200>, 50, 20>,
                    1,
                >::new(
                    1000.0, [50.0]
                )),
                downsampler: Chain::new(DownSampler::new())
                    .append(DownSampler::new())
                    .append(DownSampler::new()),
            })
        }

        match self {
            Self::Ecg(ecg) => ecg,
            _ => unreachable!(),
        }
    }
}

pub static mut BIG_OBJECTS: BigObjects = BigObjects::Unused;
