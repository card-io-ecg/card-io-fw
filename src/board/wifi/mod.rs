use crate::{
    board::{
        hal::{
            clock::Clocks,
            peripherals::{RNG, TIMG1},
            radio::Wifi,
            system::{PeripheralClockControl, RadioClockControl},
            timer::{Timer0, TimerGroup},
            Rng, Timer,
        },
        wifi::ap::ApState,
    },
    replace_with::replace_with_or_abort_async,
    task_control::TaskController,
};
use embassy_net::{Config, Stack, StackResources};
use embedded_hal_old::prelude::_embedded_hal_blocking_rng_Read;
use esp_wifi::{wifi::WifiDevice, EspWifiInitFor, EspWifiInitialization};
use rand_core::{RngCore, SeedableRng};
use replace_with::replace_with_or_abort;
use wyhash::WyRng;

pub unsafe fn as_static_ref<T>(what: &T) -> &'static T {
    core::mem::transmute(what)
}

pub unsafe fn as_static_mut<T>(what: &mut T) -> &'static mut T {
    core::mem::transmute(what)
}

pub mod ap;

pub struct WifiDriver {
    wifi: Wifi,
    rng: WyRng,
    state: WifiDriverState,
}

#[allow(clippy::large_enum_variant)]
enum WifiDriverState {
    Uninitialized {
        timer: Timer<Timer0<TIMG1>>,
        rng: Rng<'static>,
        rcc: RadioClockControl,
    },
    Initialized {
        init: EspWifiInitialization,
    },
    AP(ApState),
}

impl WifiDriverState {
    fn initialize(self, clocks: &Clocks<'_>) -> Self {
        if let WifiDriverState::Uninitialized { timer, rng, rcc } = self {
            WifiDriverState::Initialized {
                init: esp_wifi::initialize(EspWifiInitFor::Wifi, timer, rng, rcc, clocks).unwrap(),
            }
        } else {
            self
        }
    }
}

impl WifiDriver {
    pub fn new(
        wifi: Wifi,
        timer: TIMG1,
        rng: RNG,
        rcc: RadioClockControl,
        clocks: &Clocks,
        pcc: &mut PeripheralClockControl,
    ) -> Self {
        let mut rng = Rng::new(rng);
        let mut seed_bytes = [0; 8];
        rng.read(&mut seed_bytes).unwrap();
        Self {
            wifi,
            rng: WyRng::from_seed(seed_bytes),
            state: WifiDriverState::Uninitialized {
                timer: TimerGroup::new(timer, clocks, pcc).timer0,
                rng,
                rcc,
            },
        }
    }

    pub async fn configure_ap<'d>(
        &'d mut self,
        config: Config,
        resources: &'static mut StackResources<3>,
    ) -> &'d mut Stack<WifiDevice<'static>> {
        replace_with_or_abort_async(&mut self.state, |this| async {
            match this {
                WifiDriverState::Uninitialized { .. } => unreachable!(),
                WifiDriverState::Initialized { init } => WifiDriverState::AP(ApState::new(
                    init,
                    config,
                    unsafe { as_static_mut(&mut self.wifi) },
                    resources,
                    self.rng.next_u64(),
                )),
                WifiDriverState::AP { .. } => this,
            }
        })
        .await;

        match &mut self.state {
            WifiDriverState::AP(ap) => ap.start().await,

            WifiDriverState::Uninitialized { .. } | WifiDriverState::Initialized { .. } => {
                unreachable!()
            }
        }
    }

    pub fn initialize(&mut self, clocks: &Clocks) {
        replace_with_or_abort(&mut self.state, |this| this.initialize(clocks))
    }

    pub async fn ap_client_count(&self) -> u32 {
        if let WifiDriverState::AP(ap) = &self.state {
            ap.client_count().await
        } else {
            0
        }
    }

    pub async fn stop_ap(&mut self) {
        if let WifiDriverState::AP(ap) = &mut self.state {
            ap.stop().await;
        }
    }

    pub fn ap_running(&self) -> bool {
        if let WifiDriverState::AP(ap) = &self.state {
            ap.is_running()
        } else {
            false
        }
    }
}

#[embassy_executor::task]
pub async fn net_task(
    stack: &'static Stack<WifiDevice<'static>>,
    task_control: &'static TaskController<!>,
) {
    task_control.run_cancellable(stack.run()).await;
}
