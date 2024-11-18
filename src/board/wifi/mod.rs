use core::{hint::unreachable_unchecked, mem, ops::Deref, ptr::NonNull};

use crate::{
    board::wifi::{
        ap::{Ap, ApState},
        ap_sta::ApStaState,
        sta::{Sta, StaState},
    },
    task_control::TaskControlToken,
};
use alloc::{boxed::Box, rc::Rc};
use embassy_executor::Spawner;
use embassy_net::{Config, Stack, StackResources};
use esp_hal::{
    peripherals::{RADIO_CLK, RNG, WIFI},
    rng::Rng,
    timer::AnyTimer,
};
use esp_wifi::wifi::{WifiApDevice, WifiDevice, WifiDeviceMode, WifiStaDevice};
use gui::widgets::{wifi_access_point::WifiAccessPointState, wifi_client::WifiClientState};
use macros as cardio;

pub unsafe fn as_static_mut<T>(what: &mut T) -> &'static mut T {
    mem::transmute(what)
}

const STACK_SOCKET_COUNT: usize = 3;

struct StackWrapper<MODE: WifiDeviceMode>(
    NonNull<StackResources<STACK_SOCKET_COUNT>>,
    Stack<WifiDevice<'static, MODE>>,
);

impl<MODE: WifiDeviceMode + 'static> StackWrapper<MODE> {
    fn new(wifi_interface: WifiDevice<'static, MODE>, config: Config, mut rng: Rng) -> Rc<Self> {
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

impl<MODE: WifiDeviceMode> Deref for StackWrapper<MODE> {
    type Target = Stack<WifiDevice<'static, MODE>>;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.1
    }
}

impl<MODE: WifiDeviceMode> Drop for StackWrapper<MODE> {
    fn drop(&mut self) {
        unsafe {
            _ = Box::from_raw(self.0.as_mut());
        }
    }
}

pub mod ap;
pub mod ap_sta;
pub mod sta;

pub struct WifiDriver {
    wifi: WIFI,
    rng: Rng,
    state: WifiDriverState,
}

struct WifiInitResources {
    timer: AnyTimer,
    rng: Rng,
    radio_clk: RADIO_CLK,
}

enum WifiDriverState {
    Uninitialized(WifiInitResources),
    Initialized(EspWifiInitialization),
    Ap(ApState),
    Sta(StaState),
    ApSta(ApStaState),
}

impl WifiDriverState {
    async fn initialize(&mut self, callback: impl FnOnce(EspWifiInitialization) -> Self) {
        self.uninit().await;
        replace_with::replace_with_or_abort(self, |this| {
            let token = match this {
                Self::Uninitialized(resources) => {
                    info!("Initializing Wifi driver");
                    let token = unwrap!(esp_wifi::init(
                        resources.timer,
                        resources.rng,
                        resources.radio_clk,
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
                Self::ApSta(apsta) => Self::Initialized(apsta.stop().await),
                state => state,
            };
            core::ptr::write(self, new);
        }
    }
}

impl WifiDriver {
    pub fn new(wifi: WIFI, timer: AnyTimer, rng: RNG, radio_clk: RADIO_CLK) -> Self {
        let rng = Rng::new(rng);
        Self {
            wifi,
            rng,
            state: WifiDriverState::Uninitialized(WifiInitResources {
                timer,
                rng,
                radio_clk,
            }),
        }
    }

    pub async fn configure_ap(&mut self, ap_config: Config) -> Ap {
        // Prepare, stop STA if running
        if !matches!(self.state, WifiDriverState::Ap(_)) {
            let spawner = Spawner::for_current_executor().await;
            self.state
                .initialize(|init| {
                    WifiDriverState::Ap(ApState::init(
                        init,
                        ap_config,
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

    pub async fn configure_ap_sta(&mut self, ap_config: Config, sta_config: Config) -> (Ap, Sta) {
        // Prepare, stop STA if running
        if !matches!(self.state, WifiDriverState::ApSta(_)) {
            let spawner = Spawner::for_current_executor().await;
            self.state
                .initialize(|init| {
                    WifiDriverState::ApSta(ApStaState::init(
                        init,
                        ap_config,
                        sta_config,
                        unsafe { as_static_mut(&mut self.wifi) },
                        self.rng,
                        spawner,
                    ))
                })
                .await;
        };

        if let WifiDriverState::ApSta(apsta) = &self.state {
            let (ap, sta) = apsta.handles();
            (ap.clone(), sta.clone())
        } else {
            unsafe { unreachable_unchecked() }
        }
    }

    pub async fn configure_sta(&mut self, sta_config: Config) -> Sta {
        // Prepare, stop AP if running
        if !matches!(self.state, WifiDriverState::Sta(_)) {
            let spawner = Spawner::for_current_executor().await;
            self.state
                .initialize(|init| {
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
            WifiDriverState::ApSta(ap_sta) => Some(&ap_sta.handles().0),
            _ => None,
        }
    }

    pub fn sta_handle(&self) -> Option<&Sta> {
        match &self.state {
            WifiDriverState::Sta(sta) => Some(sta.handle()),
            WifiDriverState::ApSta(ap_sta) => Some(&ap_sta.handles().1),
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
async fn ap_net_task(stack: Rc<StackWrapper<WifiApDevice>>, mut task_control: TaskControlToken<!>) {
    task_control.run_cancellable(|_| stack.run()).await;
}

#[cardio::task]
async fn sta_net_task(
    stack: Rc<StackWrapper<WifiStaDevice>>,
    mut task_control: TaskControlToken<!>,
) {
    task_control.run_cancellable(|_| stack.run()).await;
}
