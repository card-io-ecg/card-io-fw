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
use alloc::boxed::Box;
use embassy_net::{Config, Stack, StackResources};
use esp_wifi::{wifi::WifiDevice, EspWifiInitFor, EspWifiInitialization};

pub unsafe fn as_static_ref<T>(what: &T) -> &'static T {
    mem::transmute(what)
}

pub unsafe fn as_static_mut<T>(what: &mut T) -> &'static mut T {
    mem::transmute(what)
}

pub mod ap;

pub struct WifiDriver {
    wifi: Wifi,
    rng: Rng,
    state: WifiDriverState,
}

struct WifiInitResources {
    timer: Timer<Timer0<TIMG1>>,
    rng: Rng,
    rcc: RadioClockControl,
}

#[allow(clippy::large_enum_variant)]
enum WifiDriverState {
    Uninitialized(WifiInitResources),
    Initialized(EspWifiInitialization),
    AP(MaybeUninit<ApState>, Box<StackResources<3>>),
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
        mem::replace(
            self,
            Self::AP(MaybeUninit::uninit(), Box::new(StackResources::new())),
        )
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
        let rng = Rng::new(rng);
        Self {
            wifi,
            rng,
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
    ) -> &'d mut Stack<WifiDevice<'static>> {
        // Init AP mode
        let init = match &mut self.state {
            WifiDriverState::Uninitialized(_) => unreachable!(),
            WifiDriverState::AP(_, _) => None,
            WifiDriverState::Initialized(_) => {
                if let WifiDriverState::Initialized(init) = self.state.replace_with_ap() {
                    Some(init)
                } else {
                    unsafe { unreachable_unchecked() }
                }
            }
        };

        match &mut self.state {
            WifiDriverState::AP(ap, resources) => {
                // Initialize the memory if we need to
                if let Some(init) = init {
                    ApState::init(
                        ap,
                        init,
                        config,
                        unsafe { as_static_mut(&mut self.wifi) },
                        unsafe { as_static_mut(resources) },
                        self.rng,
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
        if let WifiDriverState::AP(ap, _) = &self.state {
            let ap = unsafe { ap.assume_init_ref() };
            ap.client_count().await
        } else {
            0
        }
    }

    pub async fn stop_if(&mut self) {
        match &mut self.state {
            WifiDriverState::AP(ap, _) => {
                let ap = unsafe { ap.assume_init_mut() };
                ap.stop().await
            }
            _ => {}
        }
    }

    pub fn ap_running(&self) -> bool {
        if let WifiDriverState::AP(ap, _) = &self.state {
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
