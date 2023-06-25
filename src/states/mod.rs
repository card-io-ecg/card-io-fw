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
pub use menu::display::display_menu;
pub use menu::main::main_menu;
pub use wifi_ap::wifi_ap;

const TARGET_FPS: u32 = 100;
const MIN_FRAME_TIME: Duration = Duration::from_hz(TARGET_FPS as u64);

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

#[allow(clippy::large_enum_variant)]
pub enum BigObjects {
    Unused,
    WifiApResources {
        resources: [WebserverResources; WEBSERVER_TASKS],
        stack_resources: StackResources<3>,
    },
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
}

pub static mut BIG_OBJECTS: BigObjects = BigObjects::Unused;
