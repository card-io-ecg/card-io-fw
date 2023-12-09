use crate::board::hal::{gpio::GpioPin, peripherals};

enum RtcioWakeupType {
    Disable = 0,
    LowLevel = 4,
    HighLevel = 5,
}

fn enable_gpio_wakeup<MODE, const PIN: u8>(_pin: &GpioPin<MODE, PIN>, level: RtcioWakeupType) {
    let sens = unsafe { &*peripherals::SENS::PTR };

    // TODO: disable clock when not in use
    sens.sar_peri_clk_gate_conf
        .modify(|_, w| w.iomux_clk_en().set_bit());

    let rtcio = unsafe { &*peripherals::RTC_IO::PTR };

    rtcio.pin[PIN as usize]
        .modify(|_, w| w.wakeup_enable().set_bit().int_type().variant(level as u8));
}

// Wakeup remains enabled after waking from deep sleep, so we need to disable it manually.
pub fn disable_gpio_wakeup<MODE, const PIN: u8>(pin: &GpioPin<MODE, PIN>) {
    enable_gpio_wakeup(pin, RtcioWakeupType::Disable)
}
