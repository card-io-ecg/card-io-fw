use embassy_time::{Duration, Timer};
use embedded_hal_async::digital::Wait;
use esp32s3_hal::gpio::GpioPin;

use crate::board::{pac, ChargerStatus, TouchDetect};

pub async fn enter_deep_sleep(mut wakeup_pin: TouchDetect, charger_pin: ChargerStatus) {
    wakeup_pin.wait_for_high().await.unwrap();
    Timer::after(Duration::from_millis(100)).await;

    // TODO: S2: disable brownout detector

    critical_section::with(|_cs| {
        configure_wakeup_sources(wakeup_pin, charger_pin);
        start_deep_sleep();
    })
}

fn configure_wakeup_sources(wakeup_pin: TouchDetect, charger_pin: ChargerStatus) {
    enable_gpio_pullup(&charger_pin);

    enable_gpio_wakeup(wakeup_pin);
    enable_gpio_wakeup(charger_pin);
}

fn enable_gpio_wakeup<MODE, const PIN: u8>(_pin: GpioPin<MODE, PIN>) {
    let sens = unsafe { &*pac::SENS::PTR };
    sens.sar_peri_clk_gate_conf
        .modify(|_, w| w.iomux_clk_en().set_bit());

    let rtcio = unsafe { &*pac::RTC_IO::PTR };

    #[allow(unused)]
    enum RtcioWakeupType {
        Disable = 0,
        LowLevel = 4,
        HighLevel = 5,
    }

    rtcio.pin[PIN as usize].modify(|_, w| {
        w.wakeup_enable()
            .set_bit()
            .int_type()
            .variant(RtcioWakeupType::LowLevel as u8)
    });
}

fn enable_gpio_pullup<MODE, const PIN: u8>(_pin: &GpioPin<MODE, PIN>) {
    let rtcio = unsafe { &*pac::RTC_IO::PTR };
    let rtc_ctrl = unsafe { &*pac::RTC_CNTL::PTR };

    #[allow(clippy::single_match)]
    match PIN {
        21 => {
            rtcio.rtc_pad21.modify(|_, w| w.rue().set_bit());
            rtc_ctrl.pad_hold.modify(|_, w| w.pad21_hold().set_bit())
        }
        _ => {}
    }
}

// Assumptions: S3, Quad Flash/PSRAM, 2nd core stopped
fn start_deep_sleep() {
    // TODO: flush log buffers?

    let rtc_ctrl = unsafe { &*pac::RTC_CNTL::PTR };

    rtc_ctrl.dig_pwc.modify(|_, w| w.dg_wrap_pd_en().set_bit());

    // Enter Deep Sleep
    const WAKEUP_SOURCE_GPIO: u32 = 0x4;
    rtc_ctrl
        .wakeup_state
        .modify(|_, w| w.wakeup_ena().variant(WAKEUP_SOURCE_GPIO));

    rtc_ctrl.int_clr_rtc.write(|w| {
        w.slp_reject_int_clr()
            .set_bit()
            .slp_wakeup_int_clr()
            .set_bit()
    });

    rtc_ctrl
        .int_clr_rtc
        .write(|w| unsafe { w.bits(0x003F_FFFF) });

    /* Start entry into sleep mode */
    rtc_ctrl.state0.modify(|_, w| w.sleep_en().set_bit());
}
