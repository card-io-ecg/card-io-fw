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
        wifi::{
            ap::{Ap, ApState},
            sta::{Sta, StaState},
        },
    },
    task_control::TaskControlToken,
};
use alloc::{boxed::Box, rc::Rc};
use embassy_net::{Config, Stack, StackResources};
use esp_wifi::{wifi::WifiDevice, EspWifiInitFor, EspWifiInitialization};

pub unsafe fn as_static_mut<T>(what: &mut T) -> &'static mut T {
    mem::transmute(what)
}

pub mod ap;
pub mod sta;

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
    Ap(MaybeUninit<ApState>, Box<StackResources<3>>),
    Sta(MaybeUninit<StaState>, Box<StackResources<3>>),
}

impl WifiDriverState {
    fn initialize(&mut self, clocks: &Clocks<'_>) {
        if let WifiDriverState::Uninitialized(_) = self {
            log::info!("Initializing Wifi driver");
            // The replacement value doesn't matter as we immediately overwrite it,
            // but we need to move out of the resources
            if let WifiDriverState::Uninitialized(resources) = self.replace_with(WifiMode::Ap) {
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

    async fn uninit_mode(&mut self) {
        match self {
            WifiDriverState::Sta(sta, _) => {
                {
                    let sta = unsafe {
                        // Preinit is only called immediately before initialization, which means we
                        // immediate initialize MaybeUninit data. This in turn means that we can't
                        // have uninitialized data in preinit that was created before calling this
                        // function.
                        sta.assume_init_mut()
                    };
                    sta.stop().await;
                }

                *self = Self::Initialized(unsafe {
                    // Safety: same as above
                    sta.assume_init_read().unwrap()
                });
            }

            WifiDriverState::Ap(ap, _) => {
                {
                    let ap = unsafe {
                        // Preinit is only called immediately before initialization, which means we
                        // immediate initialize MaybeUninit data. This in turn means that we can't
                        // have uninitialized data in preinit that was created before calling this
                        // function.
                        ap.assume_init_mut()
                    };
                    ap.stop().await;
                }

                *self = Self::Initialized(unsafe {
                    // Safety: same as above
                    ap.assume_init_read().unwrap()
                });
            }

            _ => {}
        }
    }

    fn replace_with(&mut self, mode: WifiMode) -> Self {
        match mode {
            WifiMode::Ap => mem::replace(
                self,
                Self::Ap(MaybeUninit::uninit(), Box::new(StackResources::new())),
            ),
            WifiMode::Sta => mem::replace(
                self,
                Self::Sta(MaybeUninit::uninit(), Box::new(StackResources::new())),
            ),
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum WifiMode {
    Ap,
    Sta,
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

    fn wifi_mode(&self) -> Option<WifiMode> {
        match self.state {
            WifiDriverState::Ap(_, _) => Some(WifiMode::Ap),
            WifiDriverState::Sta(_, _) => Some(WifiMode::Sta),
            _ => None,
        }
    }

    async fn preinit(&mut self, mode: WifiMode) -> Option<EspWifiInitialization> {
        if self.wifi_mode() == Some(mode) {
            return None;
        }

        self.state.uninit_mode().await;

        let WifiDriverState::Initialized(init) = self.state.replace_with(mode) else {
            unsafe { unreachable_unchecked() }
        };

        Some(init)
    }

    pub async fn configure_ap(&mut self, config: Config) -> Ap {
        // Prepare, stop STA if running
        let init = self.preinit(WifiMode::Ap).await;

        // Init AP mode
        match &mut self.state {
            WifiDriverState::Ap(ap, resources) => {
                // Initialize the memory if we need to
                if let Some(init) = init {
                    ap.write(ApState::init(
                        init,
                        config,
                        unsafe { as_static_mut(&mut self.wifi) },
                        unsafe { as_static_mut(resources) },
                        self.rng,
                    ));
                }

                let ap = unsafe { ap.assume_init_mut() };
                ap.start().await
            }

            WifiDriverState::Uninitialized { .. }
            | WifiDriverState::Initialized { .. }
            | WifiDriverState::Sta(_, _) => {
                unreachable!()
            }
        }
    }

    pub async fn configure_sta(&mut self, config: Config) -> Sta {
        // Prepare, stop AP if running
        let init = self.preinit(WifiMode::Sta).await;

        // Init STA mode
        match &mut self.state {
            WifiDriverState::Sta(sta, resources) => {
                // Initialize the memory if we need to
                if let Some(init) = init {
                    sta.write(StaState::init(
                        init,
                        config,
                        unsafe { as_static_mut(&mut self.wifi) },
                        unsafe { as_static_mut(resources) },
                        self.rng,
                    ));
                }

                let sta = unsafe { sta.assume_init_mut() };
                sta.start().await
            }

            WifiDriverState::Uninitialized { .. }
            | WifiDriverState::Initialized { .. }
            | WifiDriverState::Ap(_, _) => {
                unreachable!()
            }
        }
    }

    pub async fn stop_if(&mut self) {
        self.state.uninit_mode().await;
    }

    pub fn ap_running(&self) -> bool {
        if let WifiDriverState::Ap(ap, _) = &self.state {
            let ap = unsafe { ap.assume_init_ref() };
            ap.is_running()
        } else {
            false
        }
    }
}

#[embassy_executor::task]
pub async fn net_task(
    stack: Rc<Stack<WifiDevice<'static>>>,
    mut task_control: TaskControlToken<!>,
) {
    task_control.run_cancellable(stack.run()).await;
}
