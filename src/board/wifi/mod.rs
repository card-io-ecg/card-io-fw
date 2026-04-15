use core::hint::unreachable_unchecked;

use crate::{
    board::wifi::{
        ap::{Ap, ApState},
        ap_sta::ApStaState,
        sta::{Sta, StaState},
    },
    task_control::TaskControlToken,
};
use embassy_executor::Spawner;
use embassy_net::{Config, Runner, Stack, StackResources};
use esp_hal::{peripherals::WIFI, rng::Rng};
use esp_radio::wifi::{Interface, WifiController};
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
    state: WifiDriverState,
    ap_resources: &'static mut StackResources<STACK_SOCKET_COUNT>,
    sta_resources: &'static mut StackResources<STACK_SOCKET_COUNT>,
}

struct WifiInitResources {
    wifi: WIFI<'static>,
}

enum WifiDriverState {
    Uninitialized(WifiInitResources),
    Ap(ApState),
    Sta(StaState),
    ApSta(ApStaState),
}

impl WifiDriverState {
    async fn initialize(
        &mut self,
        callback: impl FnOnce(
                WifiController<'static>,
                Stack<'static>,
                Runner<'static, Interface<'static>>,
                Stack<'static>,
                Runner<'static, Interface<'static>>,
            ) -> Self
            + 'static,
        ap_resources: &'static mut StackResources<STACK_SOCKET_COUNT>,
        sta_resources: &'static mut StackResources<STACK_SOCKET_COUNT>,
    ) {
        self.uninit().await;
        replace_with::replace_with_or_abort(self, |this| {
            if let Self::Uninitialized(resources) = this {
                let rng = Rng::new();
                let upper = rng.random() as u64;
                let lower = rng.random() as u64;

                let random_seed = upper << 32 | lower;

                info!("Initializing Wifi driver");

                let (controller, interfaces) =
                    unwrap!(esp_radio::wifi::new(resources.wifi, Default::default()));

                let (ap_stack, ap_runner) = embassy_net::new(
                    interfaces.access_point,
                    Default::default(),
                    ap_resources,
                    random_seed,
                );
                let (sta_stack, sta_runner) = embassy_net::new(
                    interfaces.station,
                    Default::default(),
                    sta_resources,
                    random_seed,
                );

                callback(controller, ap_stack, ap_runner, sta_stack, sta_runner)
            } else {
                unreachable!()
            }
        });
    }

    async fn uninit(&mut self) {
        let old = core::mem::replace(
            self,
            Self::Uninitialized(WifiInitResources {
                wifi: unsafe { WIFI::steal() },
            }),
        );

        match old {
            Self::Sta(sta) => sta.stop().await,
            Self::Ap(ap) => ap.stop().await,
            Self::ApSta(apsta) => apsta.stop().await,
            _ => {}
        };
    }
}

impl WifiDriver {
    pub fn new(wifi: WIFI<'static>) -> Self {
        let ap_resources = mk_static!(
            StackResources<STACK_SOCKET_COUNT>,
            StackResources::<STACK_SOCKET_COUNT>::new()
        );
        let sta_resources = mk_static!(
            StackResources<STACK_SOCKET_COUNT>,
            StackResources::<STACK_SOCKET_COUNT>::new()
        );

        Self {
            ap_resources,
            sta_resources,
            state: WifiDriverState::Uninitialized(WifiInitResources { wifi }),
        }
    }

    #[allow(unused)]
    pub async fn configure_ap(&mut self, ap_config: Config) -> Ap {
        // Prepare, stop STA if running
        if !matches!(self.state, WifiDriverState::Ap(_)) {
            let spawner = unsafe { Spawner::for_current_executor().await };
            self.state
                .initialize(
                    move |controller, ap_stack, ap_runner, _sta_stack, _sta_runner| {
                        ap_stack.set_config_v4(ap_config.ipv4);
                        WifiDriverState::Ap(ApState::init(controller, ap_stack, ap_runner, spawner))
                    },
                    unsafe { &mut *(self.ap_resources as *mut _) },
                    unsafe { &mut *(self.sta_resources as *mut _) },
                )
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
            let spawner = unsafe { Spawner::for_current_executor().await };
            self.state
                .initialize(
                    move |controller, ap_stack, ap_runner, sta_stack, sta_runner| {
                        ap_stack.set_config_v4(ap_config.ipv4);
                        sta_stack.set_config_v4(sta_config.ipv4);
                        WifiDriverState::ApSta(ApStaState::init(
                            controller, ap_stack, ap_runner, sta_stack, sta_runner, spawner,
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
        if !matches!(self.state, WifiDriverState::Sta(_)) {
            let spawner = unsafe { Spawner::for_current_executor().await };
            self.state
                .initialize(
                    move |controller, _ap_stack, _ap_runner, sta_stack, sta_runner| {
                        sta_stack.set_config_v4(sta_config.ipv4);
                        WifiDriverState::Sta(StaState::init(
                            controller, sta_stack, sta_runner, spawner,
                        ))
                    },
                    unsafe { &mut *(self.ap_resources as *mut _) },
                    unsafe { &mut *(self.sta_resources as *mut _) },
                )
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

#[cardio::task(pool_size = 2)]
async fn net_task(
    mut runner: Runner<'static, Interface<'static>>,
    mut task_control: TaskControlToken<()>,
) {
    task_control
        .run_cancellable(|_| async {
            runner.run().await;
        })
        .await;
}
