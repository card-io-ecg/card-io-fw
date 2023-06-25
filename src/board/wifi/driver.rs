use esp32s3_hal::system::{PeripheralClockControl, RadioClockControl};
use esp_wifi::{EspWifiInitFor, EspWifiInitialization};
use replace_with::replace_with_or_abort;

use crate::board::hal::{
    clock::Clocks,
    peripherals::{RNG, TIMG1},
    radio::Wifi,
    timer::TimerGroup,
    Rng,
};

pub struct WifiDriver {
    wifi: Wifi,
    state: WifiDriverState,
}

enum WifiDriverState {
    Uninitialized {
        timer: TIMG1,
        rng: RNG,
        rcc: RadioClockControl,
    },
    Initialized {
        init: EspWifiInitialization,
    },
}

impl WifiDriver {
    pub fn new(wifi: Wifi, timer: TIMG1, rng: RNG, rcc: RadioClockControl) -> Self {
        Self {
            wifi,
            state: WifiDriverState::Uninitialized { timer, rng, rcc },
        }
    }

    pub fn driver_mut<'d>(
        &'d mut self,
        clocks: &Clocks,
        pcc: &mut PeripheralClockControl,
    ) -> (&'d mut Wifi, &'d mut EspWifiInitialization) {
        if matches!(self.state, WifiDriverState::Uninitialized { .. }) {
            self.initialize(clocks, pcc);
        }

        match &mut self.state {
            WifiDriverState::Initialized { init } => (&mut self.wifi, init),
            WifiDriverState::Uninitialized { .. } => unreachable!(),
        }
    }

    fn initialize(&mut self, clocks: &Clocks, pcc: &mut PeripheralClockControl) {
        replace_with_or_abort(&mut self.state, |this| match this {
            WifiDriverState::Uninitialized { timer, rng, rcc } => {
                let timer = TimerGroup::new(timer, clocks, pcc).timer0;

                let init =
                    esp_wifi::initialize(EspWifiInitFor::Wifi, timer, Rng::new(rng), rcc, clocks)
                        .unwrap();

                WifiDriverState::Initialized { init }
            }
            WifiDriverState::Initialized { .. } => this,
        })
    }
}
