use core::{
    hint::unreachable_unchecked,
    mem::{self, MaybeUninit},
    ops::Deref,
    ptr::NonNull,
};

use crate::{
    board::{
        hal::{
            clock::Clocks,
            peripherals::{RNG, TIMG1},
            radio::Wifi,
            system::RadioClockControl,
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
use macros as cardio;

pub unsafe fn as_static_mut<T>(what: &mut T) -> &'static mut T {
    mem::transmute(what)
}

const STACK_SOCKET_COUNT: usize = 3;

struct StackWrapper(
    NonNull<StackResources<STACK_SOCKET_COUNT>>,
    Stack<WifiDevice<'static>>,
);

impl StackWrapper {
    fn new(wifi_interface: WifiDevice<'static>, config: Config, mut rng: Rng) -> StackWrapper {
        let lower = rng.random() as u64;
        let upper = rng.random() as u64;

        let random_seed = upper << 32 | lower;

        let resources = Box::new(StackResources::new());
        let resources_ref = Box::leak(resources);

        Self(
            NonNull::from(&mut *resources_ref),
            Stack::new(wifi_interface, config, resources_ref, random_seed),
        )
    }
}

impl Deref for StackWrapper {
    type Target = Stack<WifiDevice<'static>>;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.1
    }
}

impl Drop for StackWrapper {
    fn drop(&mut self) {
        unsafe {
            _ = Box::from_raw(self.0.as_mut());
        }
    }
}

pub mod ap;
pub mod sta;

#[derive(Default, PartialEq)]
pub enum GenericConnectionState {
    Sta(sta::ConnectionState),
    Ap(ap::ApConnectionState),
    #[default]
    Disabled,
}

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

pub enum WifiHandle {
    Ap(Ap),
    Sta(Sta),
}

impl WifiHandle {
    fn connection_state(&self) -> GenericConnectionState {
        match self {
            WifiHandle::Ap(ap) => GenericConnectionState::Ap(ap.connection_state()),
            WifiHandle::Sta(sta) => GenericConnectionState::Sta(sta.connection_state()),
        }
    }
}

#[allow(clippy::large_enum_variant)]
enum WifiDriverState {
    Uninitialized(WifiInitResources),
    Initialized(EspWifiInitialization),
    Ap(MaybeUninit<ApState>),
    Sta(MaybeUninit<StaState>),
}

impl WifiDriverState {
    fn initialize(&mut self, clocks: &Clocks<'_>) {
        if let WifiDriverState::Uninitialized(_) = self {
            info!("Initializing Wifi driver");
            // The replacement value doesn't matter as we immediately overwrite it,
            // but we need to move out of the resources
            if let WifiDriverState::Uninitialized(resources) = self.replace_with(WifiMode::Ap) {
                *self = WifiDriverState::Initialized(unwrap!(esp_wifi::initialize(
                    EspWifiInitFor::Wifi,
                    resources.timer,
                    resources.rng,
                    resources.rcc,
                    clocks,
                )));
                info!("Wifi driver initialized");
            } else {
                unsafe { unreachable_unchecked() }
            }
        }
    }

    async fn uninit_mode(&mut self) {
        match self {
            WifiDriverState::Sta(sta) => {
                unsafe {
                    // Preinit is only called immediately before initialization, which means we
                    // immediate initialize MaybeUninit data. This in turn means that we can't
                    // have uninitialized data in preinit that was created before calling this
                    // function.
                    *self = Self::Initialized(sta.assume_init_read().stop().await);
                }
            }

            WifiDriverState::Ap(ap) => {
                unsafe {
                    // Preinit is only called immediately before initialization, which means we
                    // immediate initialize MaybeUninit data. This in turn means that we can't
                    // have uninitialized data in preinit that was created before calling this
                    // function.
                    *self = Self::Initialized(ap.assume_init_read().stop().await);
                }
            }

            _ => {}
        }
    }

    fn replace_with(&mut self, mode: WifiMode) -> Self {
        match mode {
            WifiMode::Ap => mem::replace(self, Self::Ap(MaybeUninit::uninit())),
            WifiMode::Sta => mem::replace(self, Self::Sta(MaybeUninit::uninit())),
        }
    }

    fn handle(&self) -> Option<WifiHandle> {
        match self {
            WifiDriverState::Sta(sta) => unsafe {
                Some(sta.assume_init_ref().handle()).map(WifiHandle::Sta)
            },
            WifiDriverState::Ap(ap) => unsafe {
                Some(ap.assume_init_ref().handle()).map(WifiHandle::Ap)
            },
            _ => None,
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum WifiMode {
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
    ) -> Self {
        let rng = Rng::new(rng);
        Self {
            wifi,
            rng,
            state: WifiDriverState::Uninitialized(WifiInitResources {
                timer: TimerGroup::new(timer, clocks).timer0,
                rng,
                rcc,
            }),
        }
    }

    pub async fn configure_ap(&mut self, config: Config, clocks: &Clocks<'_>) -> Ap {
        // Prepare, stop STA if running
        if !matches!(self.state, WifiDriverState::Ap(_)) {
            self.stop_if().await;
            self.state.initialize(clocks);
            let WifiDriverState::Initialized(init) = self.state.replace_with(WifiMode::Ap) else {
                unsafe { unreachable_unchecked() }
            };
            // Init AP mode
            self.state = WifiDriverState::Ap(MaybeUninit::new(
                ApState::init(
                    init,
                    config,
                    unsafe { as_static_mut(&mut self.wifi) },
                    self.rng,
                )
                .await,
            ));
        };

        if let WifiDriverState::Ap(ap) = &self.state {
            unsafe { ap.assume_init_ref().handle() }
        } else {
            unsafe { unreachable_unchecked() }
        }
    }

    pub async fn configure_sta(&mut self, config: Config, clocks: &Clocks<'_>) -> Sta {
        // Prepare, stop AP if running
        if !matches!(self.state, WifiDriverState::Sta(_)) {
            self.stop_if().await;
            self.state.initialize(clocks);
            let WifiDriverState::Initialized(init) = self.state.replace_with(WifiMode::Sta) else {
                unsafe { unreachable_unchecked() }
            };
            // Init STA mode
            self.state = WifiDriverState::Sta(MaybeUninit::new(
                StaState::init(
                    init,
                    config,
                    unsafe { as_static_mut(&mut self.wifi) },
                    self.rng,
                )
                .await,
            ));
        };

        if let WifiDriverState::Sta(sta) = &self.state {
            unsafe { sta.assume_init_ref().handle() }
        } else {
            unsafe { unreachable_unchecked() }
        }
    }

    pub async fn stop_if(&mut self) {
        self.state.uninit_mode().await;
    }

    pub fn ap_running(&self) -> bool {
        if let WifiDriverState::Ap(ap) = &self.state {
            let ap = unsafe { ap.assume_init_ref() };
            ap.is_running()
        } else {
            false
        }
    }

    pub fn handle(&self) -> Option<WifiHandle> {
        self.state.handle()
    }

    pub fn connection_state(&self) -> GenericConnectionState {
        self.handle()
            .map(|handle| handle.connection_state())
            .unwrap_or_default()
    }
}

#[cardio::task]
async fn net_task(stack: Rc<StackWrapper>, mut task_control: TaskControlToken<!>) {
    task_control.run_cancellable(|_| stack.run()).await;
}
