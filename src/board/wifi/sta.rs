use core::sync::atomic::Ordering;

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
use embassy_futures::{
    join::join,
    select::{select, Either},
};
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

#[derive(PartialEq, Clone, Copy)]
pub enum NetworkPreference {
    Preferred,
    Deprioritized,
}

/// A network SSID and password, with an object used to deprioritize unstable networks.
type KnownNetwork = (WifiNetwork, NetworkPreference);

#[atomic_enum::atomic_enum]
#[derive(PartialEq)]
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
    known_networks: Shared<Vec<KnownNetwork>>,
    state: Rc<AtomicConnectionState>,
}

impl Sta {
    pub fn connection_state(&self) -> ConnectionState {
        self.state.load(Ordering::Acquire)
    }

    pub async fn visible_networks(
        &self,
    ) -> MutexGuard<'_, NoopRawMutex, heapless::Vec<AccessPointInfo, SCAN_RESULTS>> {
        self.networks.lock().await
    }

    pub async fn update_known_networks(&self, networks: &[WifiNetwork]) {
        let mut known = self.known_networks.lock().await;

        known.retain(|(network, _)| networks.contains(network));
        for network in networks {
            if !known.iter().any(|(kn, _)| kn == network) {
                known.push((network.clone(), NetworkPreference::Deprioritized));
            }
        }
    }
}

pub(super) struct StaState {
    init: EspWifiInitialization,
    controller: Shared<WifiController<'static>>,
    stack: Rc<Stack<WifiDevice<'static>>>,
    networks: Shared<heapless::Vec<AccessPointInfo, SCAN_RESULTS>>,
    known_networks: Shared<Vec<KnownNetwork>>,
    state: Rc<AtomicConnectionState>,
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
            state: Rc::new(AtomicConnectionState::new(ConnectionState::NotConnected)),
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
                self.stack.clone(),
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
    known_networks: Shared<Vec<KnownNetwork>>,
    state: Rc<AtomicConnectionState>,
    stack: Rc<Stack<WifiDevice<'static>>>,
    mut task_control: TaskControlToken<()>,
) {
    const SCAN_PERIOD: Duration = Duration::from_secs(5);
    const CONNECT_RETRY_PERIOD: Duration = Duration::from_millis(100);
    const CONNECT_RETRY_COUNT: usize = 5;

    task_control
        .run_cancellable(async {
            'scan_and_connect: loop {
                state.store(ConnectionState::NotConnected, Ordering::Release);
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

                            let mut known_networks = known_networks.lock().await;

                            // Try to find a preferred network.
                            if let Some(connect_to) = select_visible_known_network(
                                &known_networks,
                                visible_networks.as_slice(),
                                NetworkPreference::Preferred,
                            ) {
                                break 'select connect_to.clone();
                            }

                            // No preferred networks in range. Try the naughty list.
                            if let Some(connect_to) = select_visible_known_network(
                                &known_networks,
                                visible_networks.as_slice(),
                                NetworkPreference::Deprioritized,
                            ) {
                                break 'select connect_to.clone();
                            }

                            // No visible known networks. Reset deprioritized networks.
                            for (_, preference) in known_networks.iter_mut() {
                                *preference = NetworkPreference::Preferred;
                            }
                        }
                        Err(err) => log::warn!("Scan failed: {err:?}"),
                    }

                    Timer::after(SCAN_PERIOD).await;
                };

                log::info!("Connecting to {}...", connect_to.ssid);
                state.store(ConnectionState::Connecting, Ordering::Release);

                controller
                    .lock()
                    .await
                    .set_configuration(&Configuration::Client(ClientConfiguration {
                        ssid: connect_to.ssid.clone(),
                        password: connect_to.pass,
                        ..Default::default()
                    }))
                    .unwrap();

                for _ in 0..CONNECT_RETRY_COUNT {
                    match controller.lock().await.connect().await {
                        Ok(_) => {
                            log::info!("Waiting to get IP address...");

                            let wait_for_ip = async {
                                loop {
                                    if let Some(config) = stack.config_v4() {
                                        log::info!("Got IP: {}", config.address);
                                        break;
                                    }
                                    Timer::after(Duration::from_millis(500)).await;
                                }
                            };

                            let wait_for_disconnect = async {
                                let mut controller = controller.lock().await;

                                controller.wait_for_event(WifiEvent::StaDisconnected).await;
                            };

                            match select(wait_for_ip, wait_for_disconnect).await {
                                Either::First(_) => {
                                    log::info!("Wifi connected!");
                                    state.store(ConnectionState::Connected, Ordering::Release);

                                    // keep pending Disconnected event to avoid a race condition
                                    controller
                                        .lock()
                                        .await
                                        .wait_for_events(WifiEvent::StaDisconnected.into(), false)
                                        .await;

                                    // TODO: figure out if we should deprioritize, retry or just loop back
                                    // to the beginning. Maybe we could use a timer?
                                    log::info!("Wifi disconnected!");
                                    state.store(ConnectionState::NotConnected, Ordering::Release);
                                    continue 'scan_and_connect;
                                }
                                Either::Second(_) => {
                                    log::info!("Wifi disconnected!");
                                    state.store(ConnectionState::NotConnected, Ordering::Release);
                                }
                            }
                        }
                        Err(e) => {
                            log::warn!("Failed to connect to wifi: {e:?}");
                            state.store(ConnectionState::NotConnected, Ordering::Release);
                            Timer::after(CONNECT_RETRY_PERIOD).await;
                        }
                    }
                }

                // If we get here, we failed to connect to the network. Deprioritize it.
                let mut known_networks = known_networks.lock().await;
                if let Some((_, preference)) = known_networks
                    .iter_mut()
                    .find(|(kn, _)| kn.ssid == connect_to.ssid)
                {
                    *preference = NetworkPreference::Deprioritized;
                }
            }
        })
        .await;
}

fn select_visible_known_network<'a>(
    known_networks: &'a [KnownNetwork],
    visible_networks: &[AccessPointInfo],
    preference: NetworkPreference,
) -> Option<&'a WifiNetwork> {
    for network in visible_networks {
        if let Some((known_network, _)) = known_networks
            .iter()
            .find(|(kn, pref)| kn.ssid == network.ssid && *pref == preference)
        {
            return Some(known_network);
        }
    }

    None
}
