#[cfg_attr(feature = "hw_v4", path = "hardware/v4.rs")]
#[cfg_attr(all(feature = "hw_v6", feature = "esp32s3"), path = "hardware/v6s3.rs")]
#[cfg_attr(all(feature = "hw_v6", feature = "esp32c6"), path = "hardware/v6c6.rs")]
#[cfg_attr(all(feature = "hw_v8", feature = "esp32s3"), path = "hardware/v8s3.rs")]
#[cfg_attr(all(feature = "hw_v8", feature = "esp32c6"), path = "hardware/v8c6.rs")]
#[cfg_attr( // We default to hw_v8/esp32c6 if no feature is selected to help rust-analyzer for example
    not(any(
        feature = "hw_v4",
        all(feature = "hw_v6", feature = "esp32s3"),
        all(feature = "hw_v6", feature = "esp32c6"),
        all(feature = "hw_v8", feature = "esp32s3"),
        all(feature = "hw_v8", feature = "esp32c6"),
    )),
    path = "hardware/v8c6.rs"
)]
pub mod hardware;

pub mod drivers;
pub mod initialized;
#[cfg(feature = "wifi")]
pub mod ota;
pub mod startup;
pub mod storage;
pub mod utils;
#[cfg(feature = "wifi")]
pub mod wifi;

#[cfg(feature = "esp-println")]
use esp_backtrace as _;
#[cfg(feature = "esp32c6")]
use esp_hal::riscv::interrupt;
#[cfg(feature = "esp32s3")]
use esp_hal::xtensa_lx::interrupt;
use esp_hal::{
    gpio::{AnyPin, Event, Input, InputConfig, WakeupConfig},
    peripherals::LPWR,
    rtc_cntl::sleep::LowPower,
};
use esp_rtos::sleep::Sleep;
#[cfg(feature = "rtt")]
use panic_rtt_target as _;

use core::sync::atomic::{AtomicU32, Ordering};
pub use hardware::*;

const EXIT_NOT_REQUESTED: u32 = 0;
const EXIT_REQUESTED: u32 = 1;
const EXIT_REQUESTED_WHILE_CHARGING: u32 = 2;

static EXIT: AtomicU32 = AtomicU32::new(EXIT_NOT_REQUESTED);
static mut SLEEP: Option<Sleep> = None;

pub fn enter_sleep(is_charging: bool) {
    let value = if is_charging {
        EXIT_REQUESTED_WHILE_CHARGING
    } else {
        EXIT_REQUESTED
    };
    EXIT.store(value, Ordering::Relaxed);
}

fn setup_sleep(lpwr: LPWR<'static>) -> extern "C" fn() -> ! {
    let sleep = esp_rtos::sleep::configure(lpwr);

    unsafe {
        SLEEP = Some(sleep);
    }

    sleep_hook
}

extern "C" fn sleep_hook() -> ! {
    #[allow(static_mut_refs)]
    let sleep = unwrap!(unsafe { SLEEP.as_mut() });

    let deep_sleep = EXIT.load(Ordering::Relaxed);
    if deep_sleep == EXIT_NOT_REQUESTED {
        (sleep.light_sleep_hook)()
    }

    interrupt::free(|| {
        let charger_event = if deep_sleep == EXIT_REQUESTED_WHILE_CHARGING {
            // Wake up momentarily when charger is disconnected
            Event::LowLevel
        } else {
            // We want to wake up when the charger is connected, or the electrodes are touched.

            // In v2, the charger status is not connected to an RTC IO pin, so we use the VBUS
            // detect pin instead. This is a high level signal when the charger is connected.
            Event::HighLevel
        };

        let mut touch = Input::new(unsafe { AnyPin::steal(TOUCH_PIN) }, InputConfig::default());
        let mut charger_pin = Input::new(
            unsafe { AnyPin::steal(VBUS_DETECT_PIN) },
            InputConfig::default(),
        );

        let wakeup = WakeupConfig::default().with_low_power_path(true);
        unwrap!(touch.apply_wakeup_config(&wakeup));
        unwrap!(charger_pin.apply_wakeup_config(&wakeup));

        let mut lpwr = LowPower::new(unsafe { LPWR::steal() });
        lpwr.clear_wakeup_deadline();

        touch.listen(Event::LowLevel);
        charger_pin.listen(charger_event);

        sleep.deep_sleep.deep_sleep()
    })
}
