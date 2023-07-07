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
        wifi::{ap::ApState, sta::StaState},
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
pub mod sta;

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
    STA(StaState),
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

    pub fn initialize(&mut self, clocks: &Clocks) {
        replace_with_or_abort(&mut self.state, |this| this.initialize(clocks))
    }

    pub async fn configure_ap<'d>(
        &'d mut self,
        config: Config,
        resources: &'static mut StackResources<3>,
    ) -> &'d mut Stack<WifiDevice<'static>> {
        replace_with_or_abort_async(&mut self.state, |this| async {
            let init = match this {
                WifiDriverState::Uninitialized { .. } => unreachable!(),
                WifiDriverState::Initialized { init } => init,
                WifiDriverState::AP(_) => return this,
                WifiDriverState::STA(sta) => sta.deinit().await,
            };

            WifiDriverState::AP(ApState::new(
                init,
                config,
                unsafe { as_static_mut(&mut self.wifi) },
                resources,
                self.rng.next_u64(),
            ))
        })
        .await;

        match &mut self.state {
            WifiDriverState::AP(ap) => ap.start().await,

            WifiDriverState::Uninitialized { .. }
            | WifiDriverState::Initialized { .. }
            | WifiDriverState::STA(_) => {
                unreachable!()
            }
        }
    }

    pub async fn configure_sta<'d>(
        &'d mut self,
        config: Config,
        resources: &'static mut StackResources<3>,
    ) -> &'d mut Stack<WifiDevice<'static>> {
        replace_with_or_abort_async(&mut self.state, |this| async {
            let init = match this {
                WifiDriverState::Uninitialized { .. } => unreachable!(),
                WifiDriverState::Initialized { init } => init,
                WifiDriverState::AP(ap) => ap.deinit().await,
                WifiDriverState::STA(_) => return this,
            };

            WifiDriverState::STA(StaState::new(
                init,
                config,
                unsafe { as_static_mut(&mut self.wifi) },
                resources,
                self.rng.next_u64(),
            ))
        })
        .await;

        match &mut self.state {
            WifiDriverState::STA(sta) => sta.start().await,

            WifiDriverState::Uninitialized { .. }
            | WifiDriverState::Initialized { .. }
            | WifiDriverState::AP(_) => {
                unreachable!()
            }
        }
    }

    pub async fn ap_client_count(&self) -> u32 {
        if let WifiDriverState::AP(ap) = &self.state {
            ap.client_count().await
        } else {
            0
        }
    }

    pub async fn stop_if(&mut self) {
        match &mut self.state {
            WifiDriverState::AP(ap) => ap.stop().await,
            WifiDriverState::STA(sta) => sta.stop().await,
            _ => {}
        }
    }

    pub fn ap_running(&self) -> bool {
        if let WifiDriverState::AP(ap) = &self.state {
            ap.is_running()
        } else {
            false
        }
    }

    pub fn sta_connected(&self) -> bool {
        if let WifiDriverState::STA(sta) = &self.state {
            sta.is_connected()
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
