use core::{hint::unreachable_unchecked, net::Ipv4Addr};

use crate::{
    board::wifi::{
        ap::{Ap, ApState},
        ap_sta::ApStaState,
        sta::{Sta, StaState},
        time_sync::TimeSync,
    },
    task_control::TaskControlToken,
};
use embassy_executor::Spawner;
use embassy_net::{Config, Runner, Stack, StackResources};
use esp_hal::{peripherals::WIFI, rng::Rng};
use esp_radio::wifi::{Interface, WifiController, WifiError};
use gui::widgets::{wifi_access_point::WifiAccessPointState, wifi_client::WifiClientState};
use macros as cardio;

macro_rules! mk_static {
    ($t:ty,$val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        #[deny(unused_attributes)]
        let x = STATIC_CELL.uninit().write($val);
        x
    }};
}

// embassy-net adds a DNS socket to every stack. The access point also holds the DHCP server, the
// DNS server and the web server connections. The station also holds the time sync socket.
const AP_SOCKET_COUNT: usize = 5;
const STA_SOCKET_COUNT: usize = 4;

pub const AP_ADDRESS: Ipv4Addr = Ipv4Addr::new(192, 168, 2, 1);
/// `AP_ADDRESS` as the host of the setup page.
pub const AP_HOST: &str = "192.168.2.1";
/// The captive portal API (RFC 8910) that DHCP names, on the setup page's server.
pub const CAPTIVE_URL: &str = "http://192.168.2.1/captive";

/// Maximum TX power in 0.25 dBm units (5 dBm). The esp-radio default of 20 dBm turns the device off.
const MAX_TX_POWER: i8 = 20;

/// Applies `config` and limits the TX power. `set_config` starts the radio, and the driver accepts
/// the TX power only after the start.
fn set_config(
    controller: &mut WifiController<'_>,
    config: &esp_radio::wifi::Config,
) -> Result<(), WifiError> {
    controller.set_config(config)?;
    controller.set_max_tx_power(MAX_TX_POWER)
}

pub mod ap;
pub mod ap_sta;
mod power;
pub mod sta;
pub mod time_sync;

pub struct WifiDriver {
    state: WifiDriverState,
    ap_resources: &'static mut StackResources<AP_SOCKET_COUNT>,
    sta_resources: &'static mut StackResources<STA_SOCKET_COUNT>,
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
                Runner<'static, Interface>,
                Stack<'static>,
                Runner<'static, Interface>,
            ) -> Self
            + 'static,
        ap_resources: &'static mut StackResources<AP_SOCKET_COUNT>,
        sta_resources: &'static mut StackResources<STA_SOCKET_COUNT>,
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

                let (ap_stack, ap_runner) = embassy_net::new(
                    esp_radio::wifi::Interface::access_point(),
                    Default::default(),
                    ap_resources,
                    random_seed,
                );
                let (sta_stack, sta_runner) = embassy_net::new(
                    esp_radio::wifi::Interface::station(),
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
            StackResources<AP_SOCKET_COUNT>,
            StackResources::<AP_SOCKET_COUNT>::new()
        );
        let sta_resources = mk_static!(
            StackResources<STA_SOCKET_COUNT>,
            StackResources::<STA_SOCKET_COUNT>::new()
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

    pub async fn configure_ap_sta(
        &mut self,
        ap_config: Config,
        sta_config: Config,
        time_sync: TimeSync,
    ) -> (Ap, Sta) {
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
                            time_sync,
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

    pub async fn configure_sta(&mut self, sta_config: Config, time_sync: TimeSync) -> Sta {
        // Prepare, stop AP if running
        if !matches!(self.state, WifiDriverState::Sta(_)) {
            let spawner = unsafe { Spawner::for_current_executor().await };
            self.state
                .initialize(
                    move |controller, _ap_stack, _ap_runner, sta_stack, sta_runner| {
                        sta_stack.set_config_v4(sta_config.ipv4);
                        WifiDriverState::Sta(StaState::init(
                            controller, sta_stack, sta_runner, spawner, time_sync,
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
            WifiDriverState::ApSta(ap_sta) => Some(ap_sta.handles().0),
            _ => None,
        }
    }

    pub fn sta_handle(&self) -> Option<&Sta> {
        match &self.state {
            WifiDriverState::Sta(sta) => Some(sta.handle()),
            WifiDriverState::ApSta(ap_sta) => Some(ap_sta.handles().1),
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

// A Wi-Fi mode spawns its tasks right after the previous mode stopped the same ones. A stopped
// task keeps its pool slot while it is in the run queue, and a stale waker can put it back there,
// so each task that a mode spawns has one slot more than a mode needs. Access point and station
// together run two of this task.
#[cardio::task(pool_size = 3)]
async fn net_task(mut runner: Runner<'static, Interface>, mut task_control: TaskControlToken<()>) {
    task_control
        .run_cancellable(|_| async {
            runner.run().await;
        })
        .await;
}
