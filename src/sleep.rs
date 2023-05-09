use crate::{pac, TouchDetect};

pub fn enter_deep_sleep(wakeup_pin: TouchDetect) -> ! {
    let rtc = unsafe { &*pac::RTC_CNTL::PTR };

    todo!()
}
