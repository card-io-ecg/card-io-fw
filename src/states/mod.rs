pub mod charging;
pub mod display_serial;
#[cfg(feature = "wifi")]
pub mod firmware_update;
pub mod init;
pub mod measure;
pub mod menu;
#[cfg(feature = "wifi")]
pub mod sync;
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

// The max number of concurrent webserver connections.
#[cfg(feature = "wifi")]
const WEBSERVER_TASKS: usize = 2;

/// Simple utility to process touch events in an interactive menu.
pub struct TouchInputShaper {
    released: bool,
    /// Whether the pin was low at the previous update.
    was_low: bool,
    /// When the pin went low and stayed low, while no touch is active.
    pressed_since: Option<Instant>,
    /// Set while touched, and kept set for `RELEASE_DEBOUNCE` after the touch is lost. Stored as
    /// a deadline instead of a counter, so that the debounce does not depend on the update rate.
    touched_until: Option<Instant>,
}

impl TouchInputShaper {
    const RELEASE_DEBOUNCE: Duration = Duration::from_millis(50);
    /// A new touch counts only if the touch pin stays low this long. Longer than one 50 Hz mains
    /// period: a charger couples mains noise into the touch detector, which pulls the pin low for
    /// a part of each period.
    const PRESS_DEBOUNCE: Duration = Duration::from_millis(30);

    pub fn new() -> Self {
        Self {
            released: false,
            was_low: false,
            pressed_since: None,
            touched_until: None,
        }
    }

    /// Starts with an active touch, for a touch that is already held, such as the one that woke
    /// the device.
    pub fn new_released() -> Self {
        Self {
            released: true,
            was_low: true,
            pressed_since: None,
            touched_until: Some(Instant::now() + Self::RELEASE_DEBOUNCE),
        }
    }

    /// Samples the touch pin. The pin counts as held only if it was low at the previous update, is
    /// low now, and did not go high in between, so noise pulses do not count.
    pub fn update(&mut self, frontend: &mut EcgFrontend) {
        let now = Instant::now();

        let (is_low, went_high) = frontend.sample_touch();
        let held = self.was_low && is_low && !went_high;
        self.was_low = is_low;

        if self.touched_until.is_some() {
            if held {
                self.touched_until = Some(now + Self::RELEASE_DEBOUNCE);
            } else if self.touched_until.is_some_and(|until| now >= until) {
                self.touched_until = None;
            }
        } else if !is_low {
            self.pressed_since = None;
        } else if !held {
            // The pin was high at some point since the previous update: restart the wait.
            self.pressed_since = Some(now);
        } else {
            let since = *self.pressed_since.get_or_insert(now);
            if now - since >= Self::PRESS_DEBOUNCE {
                self.pressed_since = None;
                self.touched_until = Some(now + Self::RELEASE_DEBOUNCE);
            }
        }

        // A touch held over from the previous screen counts only after it is lifted.
        if self.touched_until.is_none() && !is_low {
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
