#![allow(unused)]

use crate::board::{hal::gpio::GpioPin, pac};

pub enum RtcioWakeupType {
    Disable = 0,
    LowLevel = 4,
    HighLevel = 5,
}

pub fn enable_gpio_wakeup<MODE, const PIN: u8>(_pin: &GpioPin<MODE, PIN>, level: RtcioWakeupType) {
    let sens = unsafe { &*pac::SENS::PTR };

    // TODO: disable clock when not in use
    sens.sar_peri_clk_gate_conf
        .modify(|_, w| w.iomux_clk_en().set_bit());

    let rtcio = unsafe { &*pac::RTC_IO::PTR };

    rtcio.pin[PIN as usize]
        .modify(|_, w| w.wakeup_enable().set_bit().int_type().variant(level as u8));
}

// Wakeup remains enabled after waking from deep sleep, so we need to disable it manually.
pub fn disable_gpio_wakeup<MODE, const PIN: u8>(pin: &GpioPin<MODE, PIN>) {
    enable_gpio_wakeup(pin, RtcioWakeupType::Disable)
}

pub fn enable_gpio_pullup<MODE, const PIN: u8>(_pin: &GpioPin<MODE, PIN>) {
    let rtcio = unsafe { &*pac::RTC_IO::PTR };
    let rtc_ctrl = unsafe { &*pac::RTC_CNTL::PTR };

    #[allow(clippy::single_match)]
    match PIN {
        17 => {
            rtcio.pad_dac1.modify(|_, w| w.pdac1_rue().set_bit());
            rtc_ctrl.pad_hold.modify(|_, w| w.pdac1_hold().set_bit())
        }
        21 => {
            rtcio.rtc_pad21.modify(|_, w| w.rue().set_bit());
            rtc_ctrl.pad_hold.modify(|_, w| w.pad21_hold().set_bit())
        }
        _ => {}
    }
}

// Assumptions: S3, Quad Flash/PSRAM, 2nd core stopped
pub fn start_deep_sleep() {
    // TODO: S2: disable brownout detector
    // TODO: flush log buffers?

    let rtc_ctrl = unsafe { &*pac::RTC_CNTL::PTR };

    rtc_ctrl.dig_pwc.modify(|_, w| w.dg_wrap_pd_en().set_bit());
    rtc_ctrl
        .sdio_conf
        .modify(|_, w| w.sdio_reg_pd_en().set_bit());

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
