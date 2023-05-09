use crate::board::{pac, TouchDetect};

pub fn enter_deep_sleep(wakeup_pin: TouchDetect) -> ! {
    let rtc_ctrl = unsafe { &*pac::RTC_CNTL::PTR };

    todo!()
}
