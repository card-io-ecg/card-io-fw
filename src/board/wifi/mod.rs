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
use embassy_net::{
    iface::Iface,
    wire::{IpCidr, Ipv4Address},
    Runner, Stack, StackStorage,
};
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

pub mod ap;
pub mod ap_sta;
pub mod sta;

#[derive(Clone, Copy)]
pub enum Ipv4NetConfig {
    Dhcpv4,
    Static {
        address: IpCidr,
        gateway: Option<Ipv4Address>,
    },
}

pub struct WifiDriver {
    state: WifiDriverState,
    net: Option<WifiNet>,
}

struct WifiInitResources {
    wifi: WIFI<'static>,
}

#[derive(Clone, Copy)]
pub(super) struct WifiNet {
    ap_stack: Stack<'static>,
    ap_iface: Iface<'static>,
    ap_runner: *mut Runner<'static>,
    sta_stack: Stack<'static>,
    sta_iface: Iface<'static>,
    sta_runner: *mut Runner<'static>,
}

unsafe impl Send for WifiNet {}

enum WifiDriverState {
    Uninitialized(WifiInitResources),
    Ap(ApState),
    Sta(StaState),
    ApSta(ApStaState),
}

fn apply_ipv4_config(stack: Stack<'static>, iface: Iface<'static>, config: Ipv4NetConfig) {
    match config {
        Ipv4NetConfig::Dhcpv4 => {
            iface.set_dhcpv4(Some(Default::default()));
        }
        Ipv4NetConfig::Static { address, gateway } => {
            unwrap!(iface.add_ip_addr(address));
            if let Some(gateway) = gateway {
                unwrap!(stack
                    .routes()
                    .add_default_ipv4_route(gateway, iface.handle()));
            }
        }
    }
}

impl WifiDriverState {
    async fn initialize(
        &mut self,
        callback: impl FnOnce(WifiController<'static>, WifiNet) -> Self + 'static,
        net: &mut Option<WifiNet>,
    ) {
        self.uninit().await;
        replace_with::replace_with_or_abort(self, |this| {
            if let Self::Uninitialized(resources) = this {
                let rng = Rng::new();
                let upper = rng.random() as u64;
                let lower = rng.random() as u64;

                let random_seed = upper << 32 | lower;

                info!("Initializing Wifi driver");

                let controller = unwrap!(esp_radio::wifi::WifiController::new(
                    resources.wifi,
                    Default::default()
                ));

                let net = *net.get_or_insert_with(|| {
                    let ap_storage = mk_static!(StackStorage<'static>, StackStorage::new());
                    let sta_storage = mk_static!(StackStorage<'static>, StackStorage::new());
                    let ap_interface = mk_static!(Interface, Interface::access_point());
                    let sta_interface = mk_static!(Interface, Interface::station());

                    let (ap_stack, ap_runner) = Stack::new(ap_storage, random_seed);
                    let ap_iface = unwrap!(ap_stack.add_iface(ap_interface));
                    let ap_runner = mk_static!(Runner<'static>, ap_runner);

                    let (sta_stack, sta_runner) = Stack::new(sta_storage, random_seed);
                    let sta_iface = unwrap!(sta_stack.add_iface(sta_interface));
                    let sta_runner = mk_static!(Runner<'static>, sta_runner);

                    WifiNet {
                        ap_stack,
                        ap_iface,
                        ap_runner,
                        sta_stack,
                        sta_iface,
                        sta_runner,
                    }
                });

                callback(controller, net)
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
        Self {
            net: None,
            state: WifiDriverState::Uninitialized(WifiInitResources { wifi }),
        }
    }

    #[allow(unused)]
    pub async fn configure_ap(&mut self, ap_config: Ipv4NetConfig) -> Ap {
        // Prepare, stop STA if running
        if !matches!(self.state, WifiDriverState::Ap(_)) {
            let spawner = unsafe { Spawner::for_current_executor().await };
            self.state
                .initialize(
                    move |controller, net| {
                        apply_ipv4_config(net.ap_stack, net.ap_iface, ap_config);
                        WifiDriverState::Ap(ApState::init(controller, net, spawner))
                    },
                    &mut self.net,
                )
                .await;
        };

        if let WifiDriverState::Ap(ap) = &self.state {
            ap.handle().clone()
        } else {
            unsafe { unreachable_unchecked() }
        }
    }

    pub async fn configure_ap_sta(
        &mut self,
        ap_config: Ipv4NetConfig,
        sta_config: Ipv4NetConfig,
    ) -> (Ap, Sta) {
        // Prepare, stop STA if running
        if !matches!(self.state, WifiDriverState::ApSta(_)) {
            let spawner = unsafe { Spawner::for_current_executor().await };
            self.state
                .initialize(
                    move |controller, net| {
                        apply_ipv4_config(net.ap_stack, net.ap_iface, ap_config);
                        apply_ipv4_config(net.sta_stack, net.sta_iface, sta_config);
                        WifiDriverState::ApSta(ApStaState::init(controller, net, spawner))
                    },
                    &mut self.net,
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

    pub async fn configure_sta(&mut self, sta_config: Ipv4NetConfig) -> Sta {
        // Prepare, stop AP if running
        if !matches!(self.state, WifiDriverState::Sta(_)) {
            let spawner = unsafe { Spawner::for_current_executor().await };
            self.state
                .initialize(
                    move |controller, net| {
                        apply_ipv4_config(net.sta_stack, net.sta_iface, sta_config);
                        WifiDriverState::Sta(StaState::init(controller, net, spawner))
                    },
                    &mut self.net,
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
pub(super) async fn net_task(
    runner: &'static mut Runner<'static>,
    mut task_control: TaskControlToken<()>,
) {
    task_control
        .run_cancellable(|_| async {
            runner.run().await;
        })
        .await;
}
