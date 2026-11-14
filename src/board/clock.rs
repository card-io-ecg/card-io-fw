use core::sync::atomic::{AtomicBool, Ordering};

use esp_hal::{peripherals::RTC_TIMER, rtc_cntl::Rtc};

// UTC once the clock is set. Before that, the time since the last power-on reset, which the
// decoder shows as a date in 1970.
defmt::timestamp!("{=u64:iso8601ms}", rtc_time_us() / 1000);

/// The RTC time in microseconds, also while it is not known. The logger runs before `Clock`
/// exists, so this reads the timer without it.
fn rtc_time_us() -> u64 {
    // SAFETY: this only reads the timer and the boot time registers. `Clock::set_us` is the only
    // writer, and a log frame that interrupts it reads either the old or the new boot time.
    Rtc::new(unsafe { RTC_TIMER::steal() }).current_time_us()
}

/// UTC, kept by the RTC timer through sleep and through each reset except a power-on reset.
pub struct Clock {
    rtc: Rtc<'static>,
    /// Set when the time came from a server in this boot.
    synced: AtomicBool,
}

impl Clock {
    /// 2026-01-01T00:00:00Z. After a power-on reset the RTC timer counts from 1970.
    const MIN_VALID_US: u64 = 1_767_225_600_000_000;

    pub fn new(rtc_timer: RTC_TIMER<'static>) -> Self {
        Self {
            rtc: Rtc::new(rtc_timer),
            synced: AtomicBool::new(false),
        }
    }

    /// Unix time in microseconds, or `None` while the time is not known.
    pub fn now_us(&self) -> Option<u64> {
        let now = self.rtc.current_time_us();
        (now >= Self::MIN_VALID_US).then_some(now)
    }

    #[cfg_attr(not(feature = "wifi"), allow(dead_code))]
    pub fn set_us(&self, unix_us: u64) {
        self.rtc.set_current_time_us(unix_us);
        self.synced.store(true, Ordering::Relaxed);
    }

    #[cfg_attr(not(feature = "wifi"), allow(dead_code))]
    pub fn synced(&self) -> bool {
        self.synced.load(Ordering::Relaxed)
    }
}
