mod adc_setup;
mod charging;
mod error;
mod init;
mod measure;
mod menu;
mod upload_or_store_measurement;

use embassy_time::Duration;
use embedded_graphics::Drawable;

pub use adc_setup::adc_setup;
pub use charging::charging;
pub use error::app_error;
use gui::{
    screens::{message::MessageScreen, screen::Screen},
    widgets::{
        battery_small::Battery,
        status_bar::StatusBar,
        wifi::{WifiState, WifiStateView},
    },
};
pub use init::initialize;
pub use measure::{measure, ECG_BUFFER_SIZE};
#[cfg(feature = "battery_max17055")]
pub use menu::battery_info::battery_info_menu;
pub use menu::{
    about::about_menu, display::display_menu, main::main_menu, wifi_ap::wifi_ap,
    wifi_sta::wifi_sta, AppMenu,
};
pub use upload_or_store_measurement::upload_or_store_measurement;

const TARGET_FPS: u32 = 100;
const MIN_FRAME_TIME: Duration = Duration::from_hz(TARGET_FPS as u64);

const MENU_IDLE_DURATION: Duration = Duration::from_secs(30);

// The max number of webserver tasks.
const WEBSERVER_TASKS: usize = 2;

use signal_processing::lerp::interpolate;

use crate::board::{initialized::Board, wifi::GenericConnectionState, EcgFrontend};

/// Simple utility to process touch events in an interactive menu.
pub struct TouchInputShaper<'a> {
    frontend: &'a mut EcgFrontend,
    released: bool,
}

impl<'a> TouchInputShaper<'a> {
    pub fn new(frontend: &'a mut EcgFrontend) -> Self {
        Self {
            frontend,
            released: false,
        }
    }

    pub fn is_touched(&mut self) -> bool {
        let is_touched = self.frontend.is_touched();

        if !is_touched {
            self.released = true;
        }

        self.released && is_touched
    }
}

fn to_progress(elapsed: Duration, max_duration: Duration) -> u32 {
    interpolate(
        elapsed.as_millis() as u32,
        0,
        max_duration.as_millis() as u32,
        0,
        255,
    )
}

async fn display_message(board: &mut Board, message: &str) {
    let connection_state = board.connection_state();
    let battery_data = board.battery_monitor.battery_data();

    board
        .display
        .frame(|display| {
            Screen {
                content: MessageScreen { message },

                status_bar: StatusBar {
                    battery: Battery::with_style(battery_data, board.config.battery_style()),
                    wifi: WifiStateView::enabled(connection_state),
                },
            }
            .draw(display)
        })
        .await;
}

impl From<GenericConnectionState> for WifiState {
    fn from(state: GenericConnectionState) -> Self {
        match state {
            GenericConnectionState::Sta(state) => state.into(),
            GenericConnectionState::Ap(state) => state.into(),
            GenericConnectionState::Disabled => WifiState::NotConnected,
        }
    }
}
