pub mod charging;
pub mod display_serial;
#[cfg(feature = "wifi")]
pub mod firmware_update;
pub mod init;
pub mod measure;
pub mod menu;
#[cfg(feature = "wifi")]
pub mod throughput;
pub mod upload_or_store_measurement;

use crate::board::EcgFrontend;
use embassy_time::{Duration, Instant};
use signal_processing::lerp::interpolate;

pub const TARGET_FPS: u32 = 100;
pub const MIN_FRAME_TIME: Duration = Duration::from_hz(TARGET_FPS as u64);
pub const MIN_MEASURE_FRAME_TIME: Duration = Duration::from_hz(TARGET_FPS as u64);

/// Menus tick at a lower rate than the measurement screen, to let the MCU sleep for longer.
pub const MENU_FRAME_TIME: Duration = Duration::from_hz(gui::screens::MENU_FPS as u64);

pub const INIT_TIME: Duration = Duration::from_millis(3000);
pub const INIT_MENU_THRESHOLD: Duration = Duration::from_millis(1500);

pub const MENU_IDLE_DURATION: Duration = Duration::from_secs(30);
pub const MESSAGE_MIN_DURATION: Duration = Duration::from_millis(300);
pub const MESSAGE_DURATION: Duration = Duration::from_millis(1500);

// The max number of webserver tasks.
#[cfg(feature = "wifi")]
const WEBSERVER_TASKS: usize = 2;

/// Simple utility to process touch events in an interactive menu.
pub struct TouchInputShaper {
    released: bool,
    /// Set while touched, and kept set for `RELEASE_DEBOUNCE` after the touch is lost. Stored as
    /// a deadline instead of a counter, so that the debounce does not depend on the update rate.
    touched_until: Option<Instant>,
}

impl TouchInputShaper {
    const RELEASE_DEBOUNCE: Duration = Duration::from_millis(50);

    pub fn new() -> Self {
        Self {
            released: false,
            touched_until: None,
        }
    }

    pub fn new_released() -> Self {
        Self {
            released: true,
            touched_until: None,
        }
    }

    pub fn update(&mut self, frontend: &mut EcgFrontend) {
        let now = Instant::now();

        if frontend.is_touched() {
            self.touched_until = Some(now + Self::RELEASE_DEBOUNCE);
        } else if self.touched_until.is_some_and(|until| now >= until) {
            self.touched_until = None;
        }

        if self.touched_until.is_none() {
            self.released = true;
        }
    }

    pub fn is_touched(&mut self) -> bool {
        self.released && self.touched_until.is_some()
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
