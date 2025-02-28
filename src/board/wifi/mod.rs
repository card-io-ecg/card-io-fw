use core::hint::unreachable_unchecked;

use crate::board::wifi::{
    ap::{Ap, ApState},
    ap_sta::ApStaState,
    sta::{Sta, StaState},
};
use embassy_executor::Spawner;
use embassy_net::{Config, Runner, Stack, StackResources};
use esp_hal::{
    peripherals::{RADIO_CLK, RNG, WIFI},
    rng::Rng,
    timer::AnyTimer,
};
use esp_wifi::{
    wifi::{WifiController, WifiDevice},
    EspWifiController,
};
use gui::widgets::{wifi_access_point::WifiAccessPointState, wifi_client::WifiClientState};
use macros as cardio;

macro_rules! mk_static {
    ($t:ty,$val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        #[deny(unused_attributes)]
        let x = STATIC_CELL.uninit().write(($val));
        x
    }};
}

const STACK_SOCKET_COUNT: usize = 3;

pub mod ap;
pub mod ap_sta;
pub mod sta;

pub struct WifiDriver {
    rng: Rng,
    state: WifiDriverState,
    ap_resources: &'static mut StackResources<STACK_SOCKET_COUNT>,
    sta_resources: &'static mut StackResources<STACK_SOCKET_COUNT>,
}

struct WifiInitResources {
    timer: AnyTimer,
    rng: Rng,
    radio_clk: RADIO_CLK,
    wifi: WIFI,
}

enum WifiDriverState {
    Uninitialized(WifiInitResources),
    Initialized {
        controller: WifiController<'static>,
        ap_stack: Stack<'static>,
        sta_stack: Stack<'static>,
    },
    Ap(ApState, Stack<'static>),
    Sta(StaState, Stack<'static>),
    ApSta(ApStaState),
}

impl WifiDriverState {
    async fn initialize(
        &mut self,
        callback: impl FnOnce(WifiController<'static>, Stack<'static>, Stack<'static>) -> Self + 'static,
        ap_resources: &'static mut StackResources<STACK_SOCKET_COUNT>,
        sta_resources: &'static mut StackResources<STACK_SOCKET_COUNT>,
    ) {
        let spawner = Spawner::for_current_executor().await;
        self.uninit().await;
        replace_with::replace_with_or_abort(self, |this| {
            let (controller, ap_stack, sta_stack) = match this {
                Self::Uninitialized(mut resources) => {
                    let lower = resources.rng.random() as u64;
                    let upper = resources.rng.random() as u64;

                    let random_seed = upper << 32 | lower;

                    info!("Initializing Wifi driver");
                    let wifi_controller = mk_static!(
                        EspWifiController<'static>,
                        unwrap!(esp_wifi::init::<'static>(
                            resources.timer,
                            resources.rng,
                            resources.radio_clk
                        ))
                    );
                    info!("Wifi driver initialized");

                    let (controller, interfaces) =
                        unwrap!(esp_wifi::wifi::new(wifi_controller, resources.wifi));

                    let (ap_stack, ap_runner) = embassy_net::new(
                        interfaces.ap,
                        Default::default(),
                        ap_resources,
                        random_seed,
                    );
                    let (sta_stack, sta_runner) = embassy_net::new(
                        interfaces.sta,
                        Default::default(),
                        sta_resources,
                        random_seed,
                    );

                    spawner.must_spawn(net_task(ap_runner));
                    spawner.must_spawn(net_task(sta_runner));

                    (controller, ap_stack, sta_stack)
                }
                Self::Initialized {
                    controller,
                    ap_stack,
                    sta_stack,
                } => (controller, ap_stack, sta_stack),
                _ => unreachable!(),
            };

            callback(controller, ap_stack, sta_stack)
        });
    }

    async fn uninit(&mut self) {
        unsafe {
            let new = match core::ptr::read(self) {
                Self::Sta(sta, ap_stack) => {
                    let (controller, sta_stack) = sta.stop().await;
                    Self::Initialized {
                        controller,
                        ap_stack,
                        sta_stack,
                    }
                }
                Self::Ap(ap, sta_stack) => {
                    let (controller, ap_stack) = ap.stop().await;
                    Self::Initialized {
                        controller,
                        ap_stack,
                        sta_stack,
                    }
                }
                Self::ApSta(apsta) => {
                    let (controller, ap_stack, sta_stack) = apsta.stop().await;
                    Self::Initialized {
                        controller,
                        ap_stack,
                        sta_stack,
                    }
                }
                state => state,
            };
            core::ptr::write(self, new);
        }
    }
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
            rng,
            ap_resources,
            sta_resources,
            state: WifiDriverState::Uninitialized(WifiInitResources {
                timer,
                rng,
                radio_clk,
                wifi,
            }),
        }
    }

    #[allow(unused)]
    pub async fn configure_ap(&mut self, ap_config: Config) -> Ap {
        // Prepare, stop STA if running
        if !matches!(self.state, WifiDriverState::Ap(_, _)) {
            let spawner = Spawner::for_current_executor().await;
            self.state
                .initialize(
                    move |controller, ap_stack, sta_stack| {
                        ap_stack.set_config_v4(ap_config.ipv4);
                        WifiDriverState::Ap(ApState::init(controller, ap_stack, spawner), sta_stack)
                    },
                    unsafe { &mut *(self.ap_resources as *mut _) },
                    unsafe { &mut *(self.sta_resources as *mut _) },
                )
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
            let rng = self.rng.clone();
            self.state
                .initialize(
                    move |controller, ap_stack, sta_stack| {
                        ap_stack.set_config_v4(ap_config.ipv4);
                        sta_stack.set_config_v4(sta_config.ipv4);
                        WifiDriverState::ApSta(ApStaState::init(
                            controller, ap_stack, sta_stack, rng, spawner,
                        ))
                    },
                    unsafe { &mut *(self.ap_resources as *mut _) },
                    unsafe { &mut *(self.sta_resources as *mut _) },
                )
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
            let rng = self.rng.clone();
            self.state
                .initialize(
                    move |controller, ap_stack, sta_stack| {
                        sta_stack.set_config_v4(sta_config.ipv4);
                        WifiDriverState::Sta(
                            StaState::init(controller, sta_stack, rng, spawner),
                            ap_stack,
                        )
                    },
                    unsafe { &mut *(self.ap_resources as *mut _) },
                    unsafe { &mut *(self.sta_resources as *mut _) },
                )
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
async fn net_task(mut runner: Runner<'static, WifiDevice<'static>>) {
    runner.run().await;
}
