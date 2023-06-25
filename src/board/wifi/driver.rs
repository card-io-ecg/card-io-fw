use embassy_net::{Config, Stack, StackResources};
use esp32s3_hal::system::{PeripheralClockControl, RadioClockControl};
use esp_wifi::{
    wifi::{WifiController, WifiDevice, WifiMode},
    EspWifiInitFor, EspWifiInitialization,
};
use replace_with::replace_with_or_abort;

use crate::board::hal::{
    clock::Clocks,
    peripherals::{RNG, TIMG1},
    radio::Wifi,
    timer::TimerGroup,
    Rng,
};

pub unsafe fn as_static_ref<T>(what: &T) -> &'static T {
    core::mem::transmute(what)
}

pub unsafe fn as_static_mut<T>(what: &mut T) -> &'static mut T {
    core::mem::transmute(what)
}

pub struct WifiDriver {
    wifi: Wifi,
    resources: StackResources<3>,
    state: WifiDriverState,
}

#[allow(clippy::large_enum_variant)]
enum WifiDriverState {
    Uninitialized {
        timer: TIMG1,
        rng: RNG,
        rcc: RadioClockControl,
    },
    Initialized {
        init: EspWifiInitialization,
    },
    AP {
        _init: EspWifiInitialization,
        controller: WifiController<'static>,
        stack: Stack<WifiDevice<'static>>,
    },
}

impl WifiDriver {
    pub fn new(wifi: Wifi, timer: TIMG1, rng: RNG, rcc: RadioClockControl) -> Self {
        Self {
            wifi,
            resources: StackResources::new(),
            state: WifiDriverState::Uninitialized { timer, rng, rcc },
        }
    }

    pub fn configure_ap<'d>(
        &'d mut self,
        config: Config,
    ) -> (
        &'d mut Stack<WifiDevice<'static>>,
        &'d mut WifiController<'static>,
    ) {
        replace_with_or_abort(&mut self.state, |this| match this {
            WifiDriverState::Uninitialized { .. } => unreachable!(),
            WifiDriverState::Initialized { init } => {
                let (wifi_interface, controller) = esp_wifi::wifi::new_with_mode(
                    &init,
                    unsafe { as_static_mut(&mut self.wifi) },
                    WifiMode::Ap,
                );

                self.resources = StackResources::new();
                let stack = Stack::new(
                    wifi_interface,
                    config,
                    unsafe { as_static_mut(&mut self.resources) },
                    1234,
                );

                WifiDriverState::AP {
                    controller,
                    stack,
                    _init: init,
                }
            }
            WifiDriverState::AP { .. } => this,
        });

        match &mut self.state {
            WifiDriverState::Uninitialized { .. } | WifiDriverState::Initialized { .. } => {
                unreachable!()
            }
            WifiDriverState::AP {
                controller, stack, ..
            } => (stack, controller),
        }
    }

    pub fn initialize(&mut self, clocks: &Clocks, pcc: &mut PeripheralClockControl) {
        replace_with_or_abort(&mut self.state, |this| match this {
            WifiDriverState::Uninitialized { timer, rng, rcc } => {
                let timer = TimerGroup::new(timer, clocks, pcc).timer0;

                let init =
                    esp_wifi::initialize(EspWifiInitFor::Wifi, timer, Rng::new(rng), rcc, clocks)
                        .unwrap();

                WifiDriverState::Initialized { init }
            }
            _ => this,
        })
    }
}
