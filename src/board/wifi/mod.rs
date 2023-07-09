use core::{hint::unreachable_unchecked, mem::MaybeUninit};

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

struct WifiInitResources {
    timer: Timer<Timer0<TIMG1>>,
    rng: Rng<'static>,
    rcc: RadioClockControl,
}

#[allow(clippy::large_enum_variant)]
enum WifiDriverState {
    Uninitialized(WifiInitResources),
    Initialized(EspWifiInitialization),
    AP(MaybeUninit<ApState>),
    STA(MaybeUninit<StaState>),
}

impl WifiDriverState {
    fn initialize(self, clocks: &Clocks<'_>) -> Self {
        if let WifiDriverState::Uninitialized(resources) = self {
            WifiDriverState::Initialized(
                esp_wifi::initialize(
                    EspWifiInitFor::Wifi,
                    resources.timer,
                    resources.rng,
                    resources.rcc,
                    clocks,
                )
                .unwrap(),
            )
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
            state: WifiDriverState::Uninitialized(WifiInitResources {
                timer: TimerGroup::new(timer, clocks, pcc).timer0,
                rng,
                rcc,
            }),
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
        // Init AP mode
        let init = match &mut self.state {
            WifiDriverState::Uninitialized(_) => unreachable!(),
            WifiDriverState::AP(_) => None,
            WifiDriverState::Initialized(_) => {
                if let WifiDriverState::Initialized(init) =
                    core::mem::replace(&mut self.state, WifiDriverState::AP(MaybeUninit::uninit()))
                {
                    Some(init)
                } else {
                    unsafe { unreachable_unchecked() }
                }
            }
            WifiDriverState::STA(sta) => {
                let sta = unsafe { sta.assume_init_mut() };
                sta.stop().await;
                if let WifiDriverState::STA(sta) =
                    core::mem::replace(&mut self.state, WifiDriverState::AP(MaybeUninit::uninit()))
                {
                    let init = unsafe { sta.assume_init().unwrap() };
                    Some(init)
                } else {
                    unsafe { unreachable_unchecked() }
                }
            }
        };

        match &mut self.state {
            WifiDriverState::AP(ap) => {
                // Initialize the memory if we need to
                if let Some(init) = init {
                    ApState::init(
                        ap,
                        init,
                        config,
                        unsafe { as_static_mut(&mut self.wifi) },
                        resources,
                        self.rng.next_u64(),
                    )
                }

                let ap = unsafe { ap.assume_init_mut() };
                ap.start().await
            }

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
        // Init STA mode
        let init = match &mut self.state {
            WifiDriverState::Uninitialized(_) => unreachable!(),
            WifiDriverState::STA(_) => None,
            WifiDriverState::Initialized(_) => {
                if let WifiDriverState::Initialized(init) =
                    core::mem::replace(&mut self.state, WifiDriverState::STA(MaybeUninit::uninit()))
                {
                    Some(init)
                } else {
                    unsafe { unreachable_unchecked() }
                }
            }
            WifiDriverState::AP(ap) => {
                let ap = unsafe { ap.assume_init_mut() };
                ap.stop().await;
                if let WifiDriverState::AP(ap) =
                    core::mem::replace(&mut self.state, WifiDriverState::STA(MaybeUninit::uninit()))
                {
                    let init = unsafe { ap.assume_init().unwrap() };
                    Some(init)
                } else {
                    unsafe { unreachable_unchecked() }
                }
            }
        };

        match &mut self.state {
            WifiDriverState::STA(sta) => {
                // Initialize the memory if we need to
                if let Some(init) = init {
                    StaState::init(
                        sta,
                        init,
                        config,
                        unsafe { as_static_mut(&mut self.wifi) },
                        resources,
                        self.rng.next_u64(),
                    )
                }

                let sta = unsafe { sta.assume_init_mut() };
                sta.start().await
            }

            WifiDriverState::Uninitialized { .. }
            | WifiDriverState::Initialized { .. }
            | WifiDriverState::AP(_) => {
                unreachable!()
            }
        }
    }

    pub async fn ap_client_count(&self) -> u32 {
        if let WifiDriverState::AP(ap) = &self.state {
            let ap = unsafe { ap.assume_init_ref() };
            ap.client_count().await
        } else {
            0
        }
    }

    pub async fn stop_if(&mut self) {
        match &mut self.state {
            WifiDriverState::AP(ap) => {
                let ap = unsafe { ap.assume_init_mut() };
                ap.stop().await
            }
            WifiDriverState::STA(sta) => {
                let sta = unsafe { sta.assume_init_mut() };
                sta.stop().await
            }
            _ => {}
        }
    }

    pub fn ap_running(&self) -> bool {
        if let WifiDriverState::AP(ap) = &self.state {
            let ap = unsafe { ap.assume_init_ref() };
            ap.is_running()
        } else {
            false
        }
    }

    pub fn sta_connected(&self) -> bool {
        if let WifiDriverState::STA(sta) = &self.state {
            let sta = unsafe { sta.assume_init_ref() };
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
