use core::cell::Cell;

use alloc::rc::Rc;
use embassy_sync::{blocking_mutex::raw::NoopRawMutex, signal::Signal};
use embassy_time::{with_timeout, Duration};
use esp_hal::rtc_cntl::WakeLock;
use esp_radio::wifi::PowerSaveMode;

const APPLY_WAIT: Duration = Duration::from_millis(500);

/// The power save state that the station task and the `Awake` guards share.
pub(super) struct Power {
    idle: PowerSaveMode,
    /// The number of live `Awake` guards.
    busy: Cell<usize>,
    changed: Signal<NoopRawMutex, ()>,
    /// The station task signals this after it applied the power save mode.
    applied: Signal<NoopRawMutex, ()>,
}

impl Power {
    pub(super) fn new(idle: PowerSaveMode) -> Self {
        Self {
            idle,
            busy: Cell::new(0),
            changed: Signal::new(),
            applied: Signal::new(),
        }
    }

    pub(super) fn wanted(&self) -> PowerSaveMode {
        if self.busy.get() > 0 {
            PowerSaveMode::None
        } else {
            self.idle
        }
    }

    pub(super) async fn wait_for_change(&self) {
        self.changed.wait().await
    }

    pub(super) fn applied(&self) {
        self.applied.signal(());
    }
}

/// Turns power save off and keeps the chip awake while it lives.
pub struct Awake {
    power: Rc<Power>,
    _lock: WakeLock,
}

impl Awake {
    /// Waits until the station task turned power save off, or a short time passed.
    pub(super) async fn new(power: Rc<Power>) -> Self {
        let awake = Self {
            power,
            _lock: WakeLock::new(),
        };
        let busy = awake.power.busy.get();
        awake.power.busy.set(busy + 1);

        // An outer guard turned power save off already.
        if busy == 0 && awake.power.idle != PowerSaveMode::None {
            awake.power.applied.reset();
            awake.power.changed.signal(());
            with_timeout(APPLY_WAIT, awake.power.applied.wait())
                .await
                .ok();
        }

        awake
    }
}

impl Drop for Awake {
    fn drop(&mut self) {
        self.power.busy.set(self.power.busy.get() - 1);
        self.power.changed.signal(());
    }
}
