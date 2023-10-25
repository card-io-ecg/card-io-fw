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
use gui::widgets::{wifi_access_point::WifiAccessPointState, wifi_client::WifiClientState};
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
            ap.handle().clone()
        } else {
            unsafe { unreachable_unchecked() }
        }
    }

    pub async fn configure_sta(&mut self, sta_config: Config, clocks: &Clocks<'_>) -> Sta {
        // Prepare, stop AP if running
        if !matches!(self.state, WifiDriverState::Sta(_)) {
            let spawner = Spawner::for_current_executor().await;
            self.state
                .initialize(clocks, |init| {
                    WifiDriverState::Sta(StaState::init(
                        init,
                        sta_config,
                        unsafe { as_static_mut(&mut self.wifi) },
                        self.rng,
                        spawner,
                    ))
                })
                .await;
        };

        if let WifiDriverState::Sta(sta) = &self.state {
            sta.handle().clone()
        } else {
            unsafe { unreachable_unchecked() }
        }
    }

    pub fn ap_handle(&self) -> Option<&Ap> {
        match &self.state {
            WifiDriverState::Ap(ap) => Some(ap.handle()),
            _ => None,
        }
    }

    pub fn sta_handle(&self) -> Option<&Sta> {
        match &self.state {
            WifiDriverState::Sta(sta) => Some(sta.handle()),
            _ => None,
        }
    }

    pub async fn stop_if(&mut self) {
        self.state.uninit().await;
    }

    pub fn ap_state(&self) -> Option<WifiAccessPointState> {
        self.ap_handle().map(|ap| ap.connection_state())
    }

    pub fn sta_state(&self) -> Option<WifiClientState> {
        self.sta_handle().map(|sta| sta.connection_state())
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
