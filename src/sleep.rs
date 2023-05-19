use embassy_time::{Duration, Timer};
use embedded_hal_async::digital::Wait;
use esp32s3_hal::reset::software_reset;

use crate::board::{pac, TouchDetect};

pub async fn enter_deep_sleep(mut wakeup_pin: TouchDetect) -> ! {
    let rtc_ctrl = unsafe { &*pac::RTC_CNTL::PTR };

    // TODO: this is a stupid simulation of sleeping
    Timer::after(Duration::from_millis(100)).await;
    wakeup_pin.wait_for_high().await.unwrap();
    wakeup_pin.wait_for_falling_edge().await.unwrap();

    software_reset();

    loop {}
}
