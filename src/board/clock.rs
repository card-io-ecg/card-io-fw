use core::sync::atomic::{AtomicBool, Ordering};

use esp_hal::{peripherals::RTC_TIMER, rtc_cntl::Rtc};

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
