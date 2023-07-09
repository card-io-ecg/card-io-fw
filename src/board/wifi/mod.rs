use core::{
    hint::unreachable_unchecked,
    mem::{self, MaybeUninit},
};

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
    task_control::TaskController,
};
use embassy_net::{Config, Stack, StackResources};
use embedded_hal_old::prelude::_embedded_hal_blocking_rng_Read;
use esp_wifi::{wifi::WifiDevice, EspWifiInitFor, EspWifiInitialization};
use rand_core::{RngCore, SeedableRng};
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
}

impl WifiDriverState {
    fn initialize(&mut self, clocks: &Clocks<'_>) {
        if let WifiDriverState::Uninitialized(_) = self {
            log::info!("Initializing Wifi driver");
            // The replacement value doesn't matter as we immediately overwrite it,
            // but we need to move out of the resources
            if let WifiDriverState::Uninitialized(resources) = self.replace_with_ap() {
                *self = WifiDriverState::Initialized(
                    esp_wifi::initialize(
                        EspWifiInitFor::Wifi,
                        resources.timer,
                        resources.rng,
                        resources.rcc,
                        clocks,
                    )
                    .unwrap(),
                );
                log::info!("Wifi driver initialized");
            } else {
                unsafe { unreachable_unchecked() }
            }
        }
    }

    fn replace_with_ap(&mut self) -> Self {
        mem::replace(self, Self::AP(MaybeUninit::uninit()))
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
        self.state.initialize(clocks);
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
                if let WifiDriverState::Initialized(init) = self.state.replace_with_ap() {
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

            WifiDriverState::Uninitialized { .. } | WifiDriverState::Initialized { .. } => {
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
}

#[embassy_executor::task]
pub async fn net_task(
    stack: &'static Stack<WifiDevice<'static>>,
    task_control: &'static TaskController<!>,
) {
    task_control.run_cancellable(stack.run()).await;
}
