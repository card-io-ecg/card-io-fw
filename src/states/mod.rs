pub mod adc_setup;
pub mod charging;
pub mod display_serial;
pub mod firmware_update;
pub mod init;
pub mod measure;
pub mod menu;
pub mod throughput;
pub mod upload_or_store_measurement;

use crate::board::{initialized::Board, wifi::GenericConnectionState, EcgFrontend};
use embassy_time::{Duration, Instant, Timer};
use embedded_graphics::Drawable;
use gui::{
    screens::{message::MessageScreen, screen::Screen},
    widgets::{
        battery_small::Battery,
        status_bar::StatusBar,
        wifi::{WifiState, WifiStateView},
    },
};
use signal_processing::lerp::interpolate;

pub const TARGET_FPS: u32 = 100;
pub const MIN_FRAME_TIME: Duration = Duration::from_hz(TARGET_FPS as u64);

pub const INIT_TIME: Duration = Duration::from_millis(3000);
pub const INIT_MENU_THRESHOLD: Duration = Duration::from_millis(1500);

pub const MENU_IDLE_DURATION: Duration = Duration::from_secs(30);
pub const MESSAGE_MIN_DURATION: Duration = Duration::from_millis(300);
pub const MESSAGE_DURATION: Duration = Duration::from_millis(1500);

// The max number of webserver tasks.
const WEBSERVER_TASKS: usize = 2;

/// Simple utility to process touch events in an interactive menu.
pub struct TouchInputShaper {
    released: bool,
    touched: bool,
    released_delay: usize,
}

impl TouchInputShaper {
    pub fn new() -> Self {
        Self {
            released: false,
            touched: false,
            released_delay: 0,
        }
    }

    pub fn new_released() -> Self {
        Self {
            released: true,
            touched: false,
            released_delay: 0,
        }
    }

    pub fn update(&mut self, frontend: &mut EcgFrontend) {
        let touched = frontend.is_touched();

        if touched {
            self.released_delay = 5;
            self.touched = true;
        } else if self.released_delay > 0 {
            self.released_delay -= 1;
        } else {
            self.touched = false;
        }

        if !self.touched {
            self.released = true;
        }
    }

    pub fn is_touched(&mut self) -> bool {
        self.released && self.touched
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

pub async fn display_message(board: &mut Board, message: &str) {
    info!("Displaying message: {}", message);

    if let Some(previous) = board.message_displayed_at.take() {
        Timer::at(previous + MESSAGE_MIN_DURATION).await;
    }

    board.message_displayed_at = Some(Instant::now());

    let status_bar = board.status_bar();
    board
        .display
        .frame(|display| {
            Screen {
                content: MessageScreen { message },
                status_bar,
            }
            .draw(display)
        })
        .await;
}

impl Board {
    pub fn status_bar(&mut self) -> StatusBar {
        let battery_data = self.battery_monitor.battery_data();
        let connection_state = match self.wifi.connection_state() {
            GenericConnectionState::Sta(state) => Some(WifiState::from(state)),
            GenericConnectionState::Ap(state) => Some(WifiState::from(state)),
            GenericConnectionState::Disabled => None,
        };

        StatusBar {
            battery: Battery::with_style(battery_data, self.config.battery_style()),
            wifi: WifiStateView::new(connection_state),
        }
    }
}
