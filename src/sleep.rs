use core::cell::RefCell;

use crate::board::{
    hal::{
        gpio::{GpioPin, RTCPin, RtcFunction},
        peripherals,
        rtc_cntl::sleep::{RtcSleepConfig, WakeSource, WakeTriggers, WakeupLevel},
        Rtc,
    },
    pac,
};

#[allow(unused)]
enum RtcioWakeupType {
    Disable = 0,
    LowLevel = 4,
    HighLevel = 5,
}

fn enable_gpio_wakeup<MODE, const PIN: u8>(_pin: &GpioPin<MODE, PIN>, level: RtcioWakeupType) {
    let sens = unsafe { &*pac::SENS::PTR };

    // TODO: disable clock when not in use
    sens.sar_peri_clk_gate_conf
        .modify(|_, w| w.iomux_clk_en().set_bit());

    let rtcio = unsafe { &*pac::RTC_IO::PTR };

    rtcio.pin[PIN as usize]
        .modify(|_, w| w.wakeup_enable().set_bit().int_type().variant(level as u8));
}

// Wakeup remains enabled after waking from deep sleep, so we need to disable it manually.
#[allow(unused)]
pub fn disable_gpio_wakeup<MODE, const PIN: u8>(pin: &GpioPin<MODE, PIN>) {
    enable_gpio_wakeup(pin, RtcioWakeupType::Disable)
}

/// RTC_IO wakeup source
///
/// RTC_IO wakeup allows configuring any combination of RTC_IO pins with
/// arbitrary wakeup levels to wake up the chip from sleep. This wakeup source
/// can be used to wake up from both light and deep sleep.
#[allow(unused)]
pub struct RtcioWakeupSource<'a, 'b> {
    pins: RefCell<&'a mut [(&'b mut dyn RTCPin, WakeupLevel)]>,
}

impl<'a, 'b> RtcioWakeupSource<'a, 'b> {
    pub fn new(pins: &'a mut [(&'b mut dyn RTCPin, WakeupLevel)]) -> Self {
        Self {
            pins: RefCell::new(pins),
        }
    }

    fn apply_pin(&self, pin: &mut dyn RTCPin, level: WakeupLevel) {
        let rtcio = unsafe { &*peripherals::RTC_IO::PTR };

        pin.rtc_set_config(true, true, RtcFunction::Rtc);

        rtcio.pin[pin.number() as usize].modify(|_, w| {
            w.wakeup_enable().set_bit().int_type().variant(match level {
                WakeupLevel::Low => 4,
                WakeupLevel::High => 5,
            })
        });
    }
}

impl WakeSource for RtcioWakeupSource<'_, '_> {
    fn apply(&self, _rtc: &Rtc, triggers: &mut WakeTriggers, sleep_config: &mut RtcSleepConfig) {
        let mut pins = self.pins.borrow_mut();

        if pins.is_empty() {
            return;
        }

        // don't power down RTC peripherals
        sleep_config.set_rtc_peri_pd_en(false);
        triggers.set_gpio(true);

        // Since we only use RTCIO pins, we can keep deep sleep enabled.
        let sens = unsafe { &*peripherals::SENS::PTR };

        // TODO: disable clock when not in use
        sens.sar_peri_clk_gate_conf
            .modify(|_, w| w.iomux_clk_en().set_bit());

        for (pin, level) in pins.iter_mut() {
            self.apply_pin(*pin, *level);
        }
    }
}

impl Drop for RtcioWakeupSource<'_, '_> {
    fn drop(&mut self) {
        // should we have saved the pin configuration first?
        // set pin back to IO_MUX (input_enable and func have no effect when pin is sent
        // to IO_MUX)
        let mut pins = self.pins.borrow_mut();
        for (pin, _level) in pins.iter_mut() {
            pin.rtc_set_config(true, false, RtcFunction::Rtc);
        }
    }
}
