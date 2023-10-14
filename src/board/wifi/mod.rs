use core::{hint::unreachable_unchecked, mem, ops::Deref, ptr::NonNull};

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
use embassy_executor::Spawner;
use embassy_futures::select::select;
use embassy_net::{Config, Stack, StackResources};
use embassy_time::{Duration, Timer as DelayTimer};
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
    fn new(wifi_interface: WifiDevice<'static>, config: Config, mut rng: Rng) -> Rc<StackWrapper> {
        const RESOURCES: StackResources<STACK_SOCKET_COUNT> = StackResources::new();

        let lower = rng.random() as u64;
        let upper = rng.random() as u64;

        let random_seed = upper << 32 | lower;

        let resources = Box::new(RESOURCES);
        let resources_ref = Box::leak(resources);

        Rc::new(Self(
            NonNull::from(&mut *resources_ref),
            Stack::new(wifi_interface, config, resources_ref, random_seed),
        ))
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

enum WifiDriverState {
    Uninitialized(WifiInitResources),
    Initialized(EspWifiInitialization),
    Ap(ApState),
    Sta(StaState),
}

impl WifiDriverState {
    async fn initialize(
        &mut self,
        clocks: &Clocks<'_>,
        callback: impl FnOnce(EspWifiInitialization) -> Self,
    ) {
        self.uninit().await;
        replace_with::replace_with_or_abort(self, |this| {
            let token = match this {
                Self::Uninitialized(resources) => {
                    info!("Initializing Wifi driver");
                    let token = unwrap!(esp_wifi::initialize(
                        EspWifiInitFor::Wifi,
                        resources.timer,
                        resources.rng,
                        resources.rcc,
                        clocks,
                    ));
                    info!("Wifi driver initialized");
                    token
                }
                Self::Initialized(token) => token,
                _ => unreachable!(),
            };

            callback(token)
        });
    }

    async fn uninit(&mut self) {
        unsafe {
            let new = match core::ptr::read(self) {
                Self::Sta(sta) => Self::Initialized(sta.stop().await),
                Self::Ap(ap) => Self::Initialized(ap.stop().await),
                state => state,
            };
            core::ptr::write(self, new);
        }
    }

    fn handle(&self) -> Option<WifiHandle> {
        match self {
            WifiDriverState::Sta(sta) => Some(WifiHandle::Sta(sta.handle())),
            WifiDriverState::Ap(ap) => Some(WifiHandle::Ap(ap.handle())),
            _ => None,
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
            let spawner = Spawner::for_current_executor().await;
            self.state
                .initialize(clocks, |init| {
                    WifiDriverState::Ap(ApState::init(
                        init,
                        config,
                        unsafe { as_static_mut(&mut self.wifi) },
                        self.rng,
                        spawner,
                    ))
                })
                .await;
        };

        if let WifiDriverState::Ap(ap) = &self.state {
            ap.handle()
        } else {
            unsafe { unreachable_unchecked() }
        }
    }

    pub async fn configure_sta(&mut self, config: Config, clocks: &Clocks<'_>) -> Sta {
        // Prepare, stop AP if running
        if !matches!(self.state, WifiDriverState::Sta(_)) {
            let spawner = Spawner::for_current_executor().await;
            self.state
                .initialize(clocks, |init| {
                    WifiDriverState::Sta(StaState::init(
                        init,
                        config,
                        unsafe { as_static_mut(&mut self.wifi) },
                        self.rng,
                        spawner,
                    ))
                })
                .await;
        };

        if let WifiDriverState::Sta(sta) = &self.state {
            sta.handle()
        } else {
            unsafe { unreachable_unchecked() }
        }
    }

    pub async fn stop_if(&mut self) {
        self.state.uninit().await;
    }

    pub fn ap_running(&self) -> bool {
        if let WifiDriverState::Ap(ap) = &self.state {
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
    task_control
        .run_cancellable(|_| async {
            select(stack.run(), async {
                // HACK: force polling the interface in case some write operation doesn't wake it up
                loop {
                    DelayTimer::after(Duration::from_secs(1)).await;
                }
            })
            .await;
            unreachable!()
        })
        .await;
}
