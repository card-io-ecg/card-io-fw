use core::{hint::unreachable_unchecked, mem};

use crate::{
    board::wifi::{
        ap::{Ap, ApState},
        ap_sta::ApStaState,
        sta::{Sta, StaState},
    },
    task_control::TaskControlToken,
};
use embassy_executor::Spawner;
use embassy_net::{Config, Runner, StackResources};
use esp_hal::{
    peripherals::{RADIO_CLK, RNG, WIFI},
    rng::Rng,
    timer::AnyTimer,
};
use esp_wifi::{wifi::WifiDevice, EspWifiController};
use gui::widgets::{wifi_access_point::WifiAccessPointState, wifi_client::WifiClientState};
use macros as cardio;

pub unsafe fn as_static_mut<T>(what: &mut T) -> &'static mut T {
    mem::transmute(what)
}

const STACK_SOCKET_COUNT: usize = 3;

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
    ap_resources: &'static mut StackResources<STACK_SOCKET_COUNT>,
    sta_resources: &'static mut StackResources<STACK_SOCKET_COUNT>,
}

enum WifiDriverState {
    Uninitialized(WifiInitResources),
    Initialized {
        controller: EspWifiController<'static>,
        ap_resources: &'static mut StackResources<STACK_SOCKET_COUNT>,
        sta_resources: &'static mut StackResources<STACK_SOCKET_COUNT>,
    },
    Ap(ApState, &'static mut StackResources<STACK_SOCKET_COUNT>),
    Sta(StaState, &'static mut StackResources<STACK_SOCKET_COUNT>),
    ApSta(ApStaState),
}

impl WifiDriverState {
    async fn initialize(
        &mut self,
        callback: impl FnOnce(
                EspWifiController<'static>,
                &'static mut StackResources<STACK_SOCKET_COUNT>,
                &'static mut StackResources<STACK_SOCKET_COUNT>,
            ) -> Self
            + 'static,
    ) {
        self.uninit().await;
        replace_with::replace_with_or_abort(self, |this| {
            let (controller, ap_resources, sta_resources) = match this {
                Self::Uninitialized(resources) => {
                    info!("Initializing Wifi driver");
                    let token = unwrap!(esp_wifi::init(
                        resources.timer,
                        resources.rng,
                        resources.radio_clk,
                    ));
                    info!("Wifi driver initialized");
                    // FIXME: this is not safe at all, but I can't be bothered to rearchitect
                    // this firmware.
                    let token = unsafe { core::mem::transmute(token) };

                    (token, resources.ap_resources, resources.sta_resources)
                }
                Self::Initialized {
                    controller,
                    ap_resources,
                    sta_resources,
                } => (controller, ap_resources, sta_resources),
                _ => unreachable!(),
            };

            callback(controller, ap_resources, sta_resources)
        });
    }

    async fn uninit(&mut self) {
        unsafe {
            let new = match core::ptr::read(self) {
                Self::Sta(sta, ap_resources) => {
                    let (controller, sta_resources) = sta.stop().await;
                    Self::Initialized {
                        controller,
                        ap_resources,
                        sta_resources,
                    }
                }
                Self::Ap(ap, sta_resources) => {
                    let (controller, ap_resources) = ap.stop().await;
                    Self::Initialized {
                        controller,
                        ap_resources,
                        sta_resources,
                    }
                }
                Self::ApSta(apsta) => {
                    let (controller, ap_resources, sta_resources) = apsta.stop().await;
                    Self::Initialized {
                        controller,
                        ap_resources,
                        sta_resources,
                    }
                }
                state => state,
            };
            core::ptr::write(self, new);
        }
    }
}

macro_rules! mk_static {
    ($t:ty,$val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        #[deny(unused_attributes)]
        let x = STATIC_CELL.uninit().write(($val));
        x
    }};
}

impl WifiDriver {
    pub fn new(wifi: WIFI, timer: AnyTimer, rng: RNG, radio_clk: RADIO_CLK) -> Self {
        let rng = Rng::new(rng);

        let ap_resources = mk_static!(
            StackResources<STACK_SOCKET_COUNT>,
            StackResources::<STACK_SOCKET_COUNT>::new()
        );
        let sta_resources = mk_static!(
            StackResources<STACK_SOCKET_COUNT>,
            StackResources::<STACK_SOCKET_COUNT>::new()
        );

        Self {
            wifi,
            rng,
            state: WifiDriverState::Uninitialized(WifiInitResources {
                timer,
                rng,
                radio_clk,
                ap_resources,
                sta_resources,
            }),
        }
    }

    #[allow(unused)]
    pub async fn configure_ap(&mut self, ap_config: Config) -> Ap {
        // Prepare, stop STA if running
        if !matches!(self.state, WifiDriverState::Ap(_, _)) {
            let spawner = Spawner::for_current_executor().await;
            let wifi = unsafe { as_static_mut(&mut self.wifi) };
            let rng = self.rng.clone();
            self.state
                .initialize(move |init, sta_resources, ap_resources| {
                    WifiDriverState::Ap(
                        ApState::init(init, ap_config, wifi, rng, ap_resources, spawner),
                        sta_resources,
                    )
                })
                .await;
        };

        if let WifiDriverState::Ap(ap, _) = &self.state {
            ap.handle().clone()
        } else {
            unsafe { unreachable_unchecked() }
        }
    }

    pub async fn configure_ap_sta(&mut self, ap_config: Config, sta_config: Config) -> (Ap, Sta) {
        // Prepare, stop STA if running
        if !matches!(self.state, WifiDriverState::ApSta(_)) {
            let spawner = Spawner::for_current_executor().await;
            let wifi = unsafe { as_static_mut(&mut self.wifi) };
            let rng = self.rng.clone();
            self.state
                .initialize(move |init, sta_resources, ap_resources| {
                    WifiDriverState::ApSta(ApStaState::init(
                        init,
                        ap_config,
                        sta_config,
                        wifi,
                        rng,
                        sta_resources,
                        ap_resources,
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
        if !matches!(self.state, WifiDriverState::Sta(_, _)) {
            let spawner = Spawner::for_current_executor().await;
            let wifi = unsafe { as_static_mut(&mut self.wifi) };
            let rng = self.rng.clone();
            self.state
                .initialize(move |init, sta_resources, ap_resources| {
                    WifiDriverState::Sta(
                        StaState::init(init, sta_config, wifi, rng, sta_resources, spawner),
                        ap_resources,
                    )
                })
                .await;
        };

        if let WifiDriverState::Sta(sta, _) = &self.state {
            sta.handle().clone()
        } else {
            unsafe { unreachable_unchecked() }
        }
    }

    pub fn ap_handle(&self) -> Option<&Ap> {
        match &self.state {
            WifiDriverState::Ap(ap, _) => Some(ap.handle()),
            WifiDriverState::ApSta(ap_sta) => Some(&ap_sta.handles().0),
            _ => None,
        }
    }

    pub fn sta_handle(&self) -> Option<&Sta> {
        match &self.state {
            WifiDriverState::Sta(sta, _) => Some(sta.handle()),
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

#[cardio::task(pool_size = 2)]
async fn net_task(
    mut runner: Runner<'static, WifiDevice<'static>>,
    mut task_control: TaskControlToken<!>,
) {
    task_control.run_cancellable(|_| runner.run()).await;
}
