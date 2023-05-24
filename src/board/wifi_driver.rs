use esp32s3_hal::system::{PeripheralClockControl, RadioClockControl};
use replace_with::replace_with_or_abort;

use crate::board::hal::{
    clock::Clocks,
    peripherals::{RNG, TIMG1},
    radio::Wifi,
    timer::TimerGroup,
    Rng,
};

pub enum WifiDriver {
    Uninitialized {
        wifi: Wifi,
        timer: TIMG1,
        rng: RNG,
        rcc: RadioClockControl,
    },
    Initialized {
        wifi: Wifi,
    },
}

impl WifiDriver {
    pub fn driver_mut<'d>(
        &'d mut self,
        clocks: &Clocks,
        pcc: &mut PeripheralClockControl,
    ) -> &'d mut Wifi {
        if !matches!(self, Self::Initialized { .. }) {
            self.initialize(clocks, pcc);
        }

        match self {
            WifiDriver::Initialized { wifi, .. } => wifi,
            WifiDriver::Uninitialized { .. } => unreachable!(),
        }
    }

    fn initialize(&mut self, clocks: &Clocks, pcc: &mut PeripheralClockControl) {
        replace_with_or_abort(self, |this| match this {
            WifiDriver::Uninitialized {
                wifi,
                timer,
                rng,
                rcc,
            } => {
                let timer = TimerGroup::new(timer, clocks, pcc).timer0;

                esp_wifi::initialize(timer, Rng::new(rng), rcc, &clocks).unwrap();

                WifiDriver::Initialized { wifi }
            }
            WifiDriver::Initialized { .. } => this,
        })
    }
}