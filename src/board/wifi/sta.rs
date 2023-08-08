use crate::{
    board::{
        hal::{radio::Wifi, Rng},
        wifi::net_task,
    },
    task_control::{TaskControlToken, TaskController},
};
use alloc::{boxed::Box, rc::Rc, vec::Vec};
use config_site::data::network::WifiNetwork;
use embassy_executor::Spawner;
use embassy_futures::join::join;
use embassy_net::{Config, Stack, StackResources};
use embassy_sync::{
    blocking_mutex::raw::NoopRawMutex,
    mutex::{Mutex, MutexGuard},
};
use embassy_time::{Duration, Timer};
use embedded_hal_old::prelude::_embedded_hal_blocking_rng_Read;
use embedded_svc::wifi::{AccessPointInfo, ClientConfiguration, Configuration, Wifi as _};
use esp_wifi::{
    wifi::{WifiController, WifiDevice, WifiEvent, WifiMode},
    EspWifiInitialization,
};
use gui::widgets::wifi::WifiState;

const SCAN_RESULTS: usize = 20;

type Shared<T> = Rc<Mutex<NoopRawMutex, T>>;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ConnectionState {
    NotConnected,
    Connecting,
    Connected,
}

impl From<ConnectionState> for WifiState {
    fn from(state: ConnectionState) -> Self {
        match state {
            ConnectionState::NotConnected => WifiState::NotConnected,
            ConnectionState::Connecting => WifiState::Connecting,
            ConnectionState::Connected => WifiState::Connected,
        }
    }
}

#[derive(Clone)]
pub struct Sta {
    _stack: Rc<Stack<WifiDevice<'static>>>,
    networks: Shared<heapless::Vec<AccessPointInfo, SCAN_RESULTS>>,
    known_networks: Shared<Vec<WifiNetwork>>,
    state: Shared<ConnectionState>,
}

impl Sta {
    pub async fn connection_state(&self) -> ConnectionState {
        *self.state.lock().await
    }

    pub async fn visible_networks(
        &self,
    ) -> MutexGuard<'_, NoopRawMutex, heapless::Vec<AccessPointInfo, SCAN_RESULTS>> {
        self.networks.lock().await
    }

    pub async fn update_known_networks(&self, networks: &[WifiNetwork]) {
        let mut known = self.known_networks.lock().await;

        known.clear();
        known.extend_from_slice(networks);
    }
}

pub(super) struct StaState {
    init: EspWifiInitialization,
    controller: Shared<WifiController<'static>>,
    stack: Rc<Stack<WifiDevice<'static>>>,
    networks: Shared<heapless::Vec<AccessPointInfo, SCAN_RESULTS>>,
    known_networks: Shared<Vec<WifiNetwork>>,
    state: Shared<ConnectionState>,
    connection_task_control: TaskController<()>,
    net_task_control: TaskController<!>,
    started: bool,
}

impl StaState {
    pub(super) fn init(
        init: EspWifiInitialization,
        config: Config,
        wifi: &'static mut Wifi,
        resources: &'static mut StackResources<3>,
        mut rng: Rng,
    ) -> Self {
        log::info!("Configuring STA");

        let (wifi_interface, controller) =
            esp_wifi::wifi::new_with_mode(&init, wifi, WifiMode::Sta).unwrap();

        let mut seed = [0; 8];
        rng.read(&mut seed).unwrap();
        let random_seed = u64::from_le_bytes(seed);

        Self {
            init,
            controller: Rc::new(Mutex::new(controller)),
            stack: Rc::new(Stack::new(wifi_interface, config, resources, random_seed)),
            networks: Rc::new(Mutex::new(heapless::Vec::new())),
            known_networks: Rc::new(Mutex::new(Vec::new())),
            state: Rc::new(Mutex::new(ConnectionState::NotConnected)),
            connection_task_control: TaskController::new(),
            net_task_control: TaskController::new(),
            started: false,
        }
    }

    pub(super) fn unwrap(self) -> EspWifiInitialization {
        self.init
    }

    pub(super) async fn stop(&mut self) {
        if self.started {
            log::info!("Stopping STA");
            let _ = join(
                self.connection_task_control.stop_from_outside(),
                self.net_task_control.stop_from_outside(),
            )
            .await;

            if matches!(self.controller.lock().await.is_started(), Ok(true)) {
                self.controller.lock().await.stop().await.unwrap();
            }

            log::info!("Stopped STA");
            self.started = false;
        }
    }

    pub(super) async fn start(&mut self) -> Sta {
        if !self.started {
            log::info!("Starting STA");
            let spawner = Spawner::for_current_executor().await;

            log::info!("Starting STA task");
            spawner.must_spawn(sta_task(
                self.controller.clone(),
                self.networks.clone(),
                self.known_networks.clone(),
                self.state.clone(),
                self.connection_task_control.token(),
            ));
            log::info!("Starting NET task");
            spawner.must_spawn(net_task(self.stack.clone(), self.net_task_control.token()));

            self.started = true;
        }

        Sta {
            _stack: self.stack.clone(),
            networks: self.networks.clone(),
            known_networks: self.known_networks.clone(),
            state: self.state.clone(),
        }
    }
}

#[embassy_executor::task]
pub(super) async fn sta_task(
    controller: Shared<WifiController<'static>>,
    networks: Shared<heapless::Vec<AccessPointInfo, SCAN_RESULTS>>,
    known_networks: Shared<Vec<WifiNetwork>>,
    state: Shared<ConnectionState>,
    mut task_control: TaskControlToken<()>,
) {
    task_control
        .run_cancellable(async {
            loop {
                *state.lock().await = ConnectionState::NotConnected;
                if !matches!(controller.lock().await.is_started(), Ok(true)) {
                    log::info!("Starting wifi");
                    controller.lock().await.start().await.unwrap();
                    log::info!("Wifi started!");
                }

                let connect_to = 'select: loop {
                    log::info!("Scanning...");

                    let mut scan_results =
                        Box::new(controller.lock().await.scan_n::<SCAN_RESULTS>().await);

                    match scan_results.as_mut() {
                        Ok((ref mut visible_networks, network_count)) => {
                            log::info!("Found {network_count} access points");

                            // Sort by signal strength, descending
                            visible_networks
                                .sort_by(|a, b| b.signal_strength.cmp(&a.signal_strength));

                            networks.lock().await.clone_from(visible_networks);

                            let known_networks = known_networks.lock().await;
                            if let Some(connect_to) = select_visible_known_network(
                                &known_networks,
                                visible_networks.as_slice(),
                            ) {
                                break 'select connect_to.clone();
                            }
                        }
                        Err(err) => log::warn!("Scan failed: {err:?}"),
                    }

                    Timer::after(Duration::from_secs(5)).await;
                };

                log::info!("Connecting...");
                *state.lock().await = ConnectionState::Connecting;

                controller
                    .lock()
                    .await
                    .set_configuration(&Configuration::Client(ClientConfiguration {
                        ssid: connect_to.ssid.clone(),
                        password: connect_to.pass.clone(),
                        ..Default::default()
                    }))
                    .unwrap();

                match controller.lock().await.connect().await {
                    Ok(_) => {
                        log::info!("Wifi connected!");
                        *state.lock().await = ConnectionState::Connected;

                        controller
                            .lock()
                            .await
                            .wait_for_event(WifiEvent::StaDisconnected)
                            .await;

                        log::info!("Wifi disconnected!");
                        *state.lock().await = ConnectionState::NotConnected;
                    }
                    Err(e) => {
                        log::warn!("Failed to connect to wifi: {e:?}");
                        *state.lock().await = ConnectionState::NotConnected;
                        Timer::after(Duration::from_millis(5000)).await
                    }
                }
            }
        })
        .await;
}

fn select_visible_known_network<'a>(
    known_networks: &'a [WifiNetwork],
    visible_networks: &[AccessPointInfo],
) -> Option<&'a WifiNetwork> {
    for network in visible_networks {
        if let Some(known_network) = known_networks.iter().find(|n| n.ssid == network.ssid) {
            return Some(known_network);
        }
    }

    None
}
