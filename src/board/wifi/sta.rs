use core::{alloc::AllocError, ptr::addr_of, sync::atomic::Ordering};

use super::STACK_SOCKET_COUNT;
use crate::{
    board::{initialized::Context, wifi::net_task},
    task_control::{TaskControlToken, TaskController},
    Shared,
};
use alloc::{boxed::Box, rc::Rc, vec::Vec};
use config_site::data::network::WifiNetwork;
use embassy_executor::Spawner;
use embassy_futures::{
    join::join,
    select::{select, Either},
};
use embassy_net::{dns::DnsSocket, Config, Stack, StackResources};
use embassy_sync::{
    blocking_mutex::raw::NoopRawMutex,
    channel::Channel,
    mutex::{Mutex, MutexGuard},
    signal::Signal,
};
use embassy_time::{with_timeout, Duration};
use enumset::EnumSet;
use esp_hal::{peripherals::WIFI, rng::Rng};
use esp_wifi::{
    wifi::{AccessPointInfo, ClientConfiguration, Configuration, WifiController, WifiEvent},
    EspWifiController,
};
use gui::widgets::wifi_client::WifiClientState;
use heapless::String;
use macros as cardio;
use reqwless::client::{HttpClient, TlsConfig, TlsVerify};

pub(super) const SCAN_RESULTS: usize = 20;

pub(super) struct StaConnectionState {
    signal: Signal<NoopRawMutex, ()>,
    value: AtomicInternalConnectionState,
}

impl StaConnectionState {
    pub fn new() -> StaConnectionState {
        Self {
            signal: Signal::new(),
            value: AtomicInternalConnectionState::new(InternalConnectionState::NotConnected),
        }
    }

    async fn wait(&self) -> InternalConnectionState {
        self.signal.wait().await;
        self.read()
    }

    fn read(&self) -> InternalConnectionState {
        self.value.load(Ordering::Acquire)
    }

    fn update(&self, value: InternalConnectionState) {
        debug!("Updating connection state: {:?}", value);
        self.value.store(value, Ordering::Release);
        self.signal.signal(());
    }
}

#[derive(PartialEq, Clone, Copy)]
pub enum NetworkPreference {
    Preferred,
    Deprioritized,
}

/// A network SSID and password, with an object used to deprioritize unstable networks.
pub type KnownNetwork = (WifiNetwork, NetworkPreference);
type Command = (StaCommand, Rc<Signal<NoopRawMutex, ()>>);
pub type CommandQueue = Channel<NoopRawMutex, Command, 1>;

#[derive(PartialEq, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[portable_atomic_enum::atomic_enum]
pub(super) enum InternalConnectionState {
    NotConnected,
    Connecting,
    WaitingForIp,
    Connected,
    Disconnected,
}

impl From<InternalConnectionState> for WifiClientState {
    fn from(value: InternalConnectionState) -> Self {
        match value {
            InternalConnectionState::NotConnected | InternalConnectionState::Disconnected => {
                WifiClientState::NotConnected
            }
            InternalConnectionState::Connecting | InternalConnectionState::WaitingForIp => {
                WifiClientState::Connecting
            }
            InternalConnectionState::Connected => WifiClientState::Connected,
        }
    }
}

#[derive(Clone)]
pub struct Sta {
    pub(super) sta_stack: Stack<'static>,
    pub(super) networks: Shared<heapless::Vec<AccessPointInfo, SCAN_RESULTS>>,
    pub(super) known_networks: Shared<Vec<KnownNetwork>>,
    pub(super) state: Rc<StaConnectionState>,
    pub(super) command_queue: Rc<CommandQueue>,
    pub(super) rng: Rng,
}

impl Sta {
    pub fn connection_state(&self) -> WifiClientState {
        self.state.read().into()
    }

    pub async fn visible_networks(
        &self,
    ) -> MutexGuard<'_, NoopRawMutex, heapless::Vec<AccessPointInfo, SCAN_RESULTS>> {
        self.networks.lock().await
    }

    pub async fn update_known_networks(&self, networks: &[WifiNetwork]) {
        let mut known = self.known_networks.lock().await;

        known.clear();
        for network in networks {
            if !known.iter().any(|(kn, _)| kn == network) {
                known.push((network.clone(), NetworkPreference::Preferred));
            }
        }
    }

    pub async fn wait_for_state_change(&self) -> WifiClientState {
        self.state.wait().await.into()
    }

    pub async fn wait_for_connection(&self, context: &mut Context) -> bool {
        if self.connection_state() != WifiClientState::Connected {
            debug!("Waiting for network connection");

            let _ = select(
                async {
                    loop {
                        let result =
                            with_timeout(Duration::from_secs(10), self.wait_for_state_change())
                                .await;
                        match result {
                            Ok(WifiClientState::Connected) => break,
                            Ok(_state) => {}
                            _ => {
                                debug!("State change timeout");
                                break;
                            }
                        }
                    }
                },
                async {
                    loop {
                        // A message is displayed for at least 300ms so we don't need to wait here.
                        context.display_message("Connecting...").await;
                    }
                },
            )
            .await;
        }

        if self.connection_state() == WifiClientState::Connected {
            true
        } else {
            debug!("No network connection");
            false
        }
    }

    /// Allocates resources for an HTTPS capable [`HttpClient`].
    pub fn https_client_resources(&self) -> Result<HttpsClientResources<'_>, AllocError> {
        // The client state must be heap allocated, because we take a reference to it.
        let resources = Box::try_new(TlsClientState::EMPTY)?;
        let client_state = unsafe { unwrap!(addr_of!(resources.tcp_state).as_ref()) };

        Ok(HttpsClientResources {
            resources,
            tcp_client: TcpClient::new(self.sta_stack.clone(), client_state),
            dns_client: DnsSocket::new(self.sta_stack.clone()),
            rng: self.rng,
        })
    }

    pub async fn send_command(&self, command: StaCommand) -> bool {
        let processed = Rc::new(Signal::new());
        if !self
            .command_queue
            .try_send((command, processed.clone()))
            .is_ok()
        {
            return false;
        }

        processed.wait().await;
        true
    }
}

const SOCKET_COUNT: usize = 1;
const SOCKET_TX_BUFFER: usize = 8 * 1024;
const SOCKET_RX_BUFFER: usize = 16 * 1024;

const TLS_READ_BUFFER: usize = 16 * 1024 + 256;
const TLS_WRITE_BUFFER: usize = 4096;

type TcpClientState =
    embassy_net::tcp::client::TcpClientState<SOCKET_COUNT, SOCKET_TX_BUFFER, SOCKET_RX_BUFFER>;
type TcpClient<'a> =
    embassy_net::tcp::client::TcpClient<'a, SOCKET_COUNT, SOCKET_TX_BUFFER, SOCKET_RX_BUFFER>;

struct TlsClientState {
    tcp_state: TcpClientState,
    tls_read_buffer: [u8; TLS_READ_BUFFER], // must be 16K
    tls_write_buffer: [u8; TLS_WRITE_BUFFER],
}

impl TlsClientState {
    pub const EMPTY: Self = Self {
        tcp_state: TcpClientState::new(),
        tls_read_buffer: [0; TLS_READ_BUFFER],
        tls_write_buffer: [0; TLS_WRITE_BUFFER],
    };
}

pub struct HttpsClientResources<'a> {
    resources: Box<TlsClientState>,
    tcp_client: TcpClient<'a>,
    dns_client: DnsSocket<'a>,
    rng: Rng,
}

impl<'a> HttpsClientResources<'a> {
    pub fn client(&mut self) -> HttpClient<'_, TcpClient<'a>, DnsSocket<'a>> {
        let upper = self.rng.random() as u64;
        let lower = self.rng.random() as u64;
        let seed = (upper << 32) | lower;

        HttpClient::new_with_tls(
            &self.tcp_client,
            &self.dns_client,
            TlsConfig::new(
                seed,
                &mut self.resources.tls_read_buffer,
                &mut self.resources.tls_write_buffer,
                TlsVerify::None,
            ),
        )
    }
}

pub(super) struct StaState {
    init: EspWifiController<'static>,
    connection_task_control: TaskController<(), StaTaskResources>,
    net_task_control: TaskController<!>,
    handle: Sta,
}

impl StaState {
    pub(super) fn init(
        init: EspWifiController<'static>,
        config: Config,
        wifi: &'static mut WIFI,
        mut rng: Rng,
        sta_resources: &'static mut StackResources<STACK_SOCKET_COUNT>,
        spawner: Spawner,
    ) -> Self {
        info!("Configuring STA");

        let (controller, interfaces) = unwrap!(esp_wifi::wifi::new(
            unsafe { core::mem::transmute(&init) },
            wifi,
        ));

        let sta_device = interfaces.sta;

        info!("Starting STA");

        let lower = rng.random() as u64;
        let upper = rng.random() as u64;

        let random_seed = upper << 32 | lower;

        let ptr = sta_resources as *mut _;
        let (sta_stack, sta_runner) =
            embassy_net::new(sta_device, config, sta_resources, random_seed);
        let networks = Rc::new(Mutex::new(heapless::Vec::new()));
        let known_networks = Rc::new(Mutex::new(Vec::new()));
        let state = Rc::new(StaConnectionState::new());
        let net_task_control = TaskController::new();
        let command_queue = Rc::new(CommandQueue::new());

        let connection_task_control = TaskController::from_resources(StaTaskResources {
            controller,
            sta_resources: ptr,
        });

        info!("Starting STA task");
        spawner.must_spawn(sta_task(
            StaController::new(
                state.clone(),
                networks.clone(),
                known_networks.clone(),
                sta_stack.clone(),
                command_queue.clone(),
                InitialStaControllerState::ScanAndConnect,
            ),
            connection_task_control.token(),
        ));

        info!("Starting NET task");
        spawner.must_spawn(net_task(sta_runner, net_task_control.token()));

        Self {
            init,
            net_task_control,
            connection_task_control,
            handle: Sta {
                sta_stack,
                networks,
                known_networks,
                state,
                command_queue,
                rng,
            },
        }
    }

    pub(super) async fn stop(
        mut self,
    ) -> (
        EspWifiController<'static>,
        &'static mut StackResources<STACK_SOCKET_COUNT>,
    ) {
        info!("Stopping STA");

        let _ = join(
            self.connection_task_control.stop(),
            self.net_task_control.stop(),
        )
        .await;

        let resources = self.connection_task_control.resources_mut().sta_resources;
        let controller = &mut self.connection_task_control.resources_mut().controller;
        if matches!(controller.is_started(), Ok(true)) {
            unwrap!(controller.stop_async().await);
        }

        info!("Stopped STA");

        (self.init, unsafe { unwrap!(resources.as_mut()) })
    }

    pub(crate) fn handle(&self) -> &Sta {
        &self.handle
    }
}

struct StaTaskResources {
    controller: WifiController<'static>,
    sta_resources: *mut StackResources<STACK_SOCKET_COUNT>,
}

unsafe impl Send for StaTaskResources {}

pub(super) enum InitialStaControllerState {
    Idle,
    ScanAndConnect,
}

impl From<InitialStaControllerState> for StaControllerState {
    fn from(value: InitialStaControllerState) -> Self {
        match value {
            InitialStaControllerState::Idle => Self::Idle,
            InitialStaControllerState::ScanAndConnect => Self::ScanAndConnect,
        }
    }
}

enum StaControllerState {
    Idle,
    ScanAndConnect,
    Connect(u8),    // select network, start connection
    AutoConnecting, // waiting for IP
    AutoConnected,  // wait for disconnection
}

const NO_TIMEOUT: Duration = Duration::MAX;
const SCAN_PERIOD: Duration = Duration::from_secs(5);
const CONTINUE: Duration = Duration::from_millis(0);
const CONNECT_RETRY_PERIOD: Duration = Duration::from_millis(100);
const CONNECT_RETRY_COUNT: u8 = 5;

pub enum StaCommand {
    ScanOnce,
}

struct ConnectError;
struct NetworkConfigureError;

pub(super) struct StaController {
    state: Rc<StaConnectionState>,
    controller_state: StaControllerState,

    networks: Shared<heapless::Vec<AccessPointInfo, SCAN_RESULTS>>,
    known_networks: Shared<Vec<KnownNetwork>>,
    stack: Stack<'static>,
    current_ssid: Option<String<32>>,

    command_queue: Rc<CommandQueue>,
}

impl StaController {
    pub fn new(
        state: Rc<StaConnectionState>,
        networks: Shared<heapless::Vec<AccessPointInfo, SCAN_RESULTS>>,
        known_networks: Shared<Vec<KnownNetwork>>,
        stack: Stack<'static>,
        command_queue: Rc<CommandQueue>,
        initial_state: InitialStaControllerState,
    ) -> Self {
        Self {
            state,
            networks,
            known_networks,
            stack,
            command_queue,
            current_ssid: None,
            controller_state: initial_state.into(),
        }
    }

    async fn setup(&mut self, controller: &mut WifiController<'_>) {
        info!("Configuring STA");

        let client_config = Configuration::Client(ClientConfiguration {
            ..Default::default()
        });
        unwrap!(controller.set_configuration(&client_config));
    }

    async fn do_scan(&mut self, controller: &mut WifiController<'_>) {
        info!("Scanning...");
        let mut scan_results = Box::new(controller.scan_n_async::<SCAN_RESULTS>().await);

        match scan_results.as_mut() {
            Ok((ref mut visible_networks, network_count)) => {
                info!("Found {} access points", network_count);

                // Sort by signal strength, descending
                visible_networks.sort_by(|a, b| b.signal_strength.cmp(&a.signal_strength));

                self.networks.lock().await.clone_from(visible_networks);
            }

            Err(err) => warn!("Scan failed: {:?}", err),
        }
    }

    async fn select_network(&self) -> Option<WifiNetwork> {
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

        let visible_networks = self.networks.lock().await;
        let mut known_networks = self.known_networks.lock().await;

        // Try to find a preferred network.
        if let Some(connect_to) = select_visible_known_network(
            &known_networks,
            visible_networks.as_slice(),
            NetworkPreference::Preferred,
        ) {
            return Some(connect_to.clone());
        }

        // No preferred networks in range. Try the naughty list.
        if let Some(connect_to) = select_visible_known_network(
            &known_networks,
            visible_networks.as_slice(),
            NetworkPreference::Deprioritized,
        ) {
            return Some(connect_to.clone());
        }

        // No visible known networks. Reset deprioritized networks.
        for (_, preference) in known_networks.iter_mut() {
            *preference = NetworkPreference::Preferred;
        }

        None
    }

    async fn configure_for_visible_network(
        &mut self,
        controller: &mut WifiController<'_>,
    ) -> Result<(), NetworkConfigureError> {
        // Select known visible network
        let Some(connect_to) = self.select_network().await else {
            return Err(NetworkConfigureError);
        };

        // Set up configuration
        info!("Connecting to {}...", connect_to.ssid);
        self.state.update(InternalConnectionState::Connecting);

        self.current_ssid = Some(connect_to.ssid.clone());

        unwrap!(
            controller.set_configuration(&Configuration::Client(ClientConfiguration {
                ssid: connect_to.ssid.clone(),
                password: connect_to.pass,
                ..Default::default()
            }))
        );

        Ok(())
    }

    async fn do_connect(
        &mut self,
        controller: &mut WifiController<'_>,
    ) -> Result<(), ConnectError> {
        self.state.update(InternalConnectionState::Connecting);
        match with_timeout(Duration::from_secs(30), controller.connect_async()).await {
            Ok(Ok(_)) => {
                self.state.update(InternalConnectionState::WaitingForIp);
                Ok(())
            }
            Ok(Err(e)) => {
                warn!("Failed to connect to wifi: {:?}", e);

                Err(ConnectError)
            }
            Err(_) => {
                warn!("Connection timeout");
                Err(ConnectError)
            }
        }
    }

    async fn deprioritize_current(&self) {
        if let Some(ssid) = self.current_ssid.as_deref() {
            let mut known_networks = self.known_networks.lock().await;
            if let Some((_, preference)) = known_networks.iter_mut().find(|(kn, preference)| {
                kn.ssid == ssid && *preference == NetworkPreference::Preferred
            }) {
                *preference = NetworkPreference::Deprioritized;
            }
        }
    }

    pub fn events(&self) -> EnumSet<WifiEvent> {
        match self.controller_state {
            StaControllerState::AutoConnecting | StaControllerState::AutoConnected => {
                enumset::enum_set! { WifiEvent::StaStop | WifiEvent::StaDisconnected }
            }
            StaControllerState::Idle
            | StaControllerState::ScanAndConnect
            | StaControllerState::Connect(_) => {
                enumset::enum_set! { WifiEvent::StaStop }
            }
        }
    }

    pub fn handle_events(&mut self, events: EnumSet<WifiEvent>) -> bool {
        if events.contains(WifiEvent::StaStop) {
            return false;
        }

        match self.controller_state {
            StaControllerState::AutoConnecting | StaControllerState::AutoConnected => {
                if events.contains(WifiEvent::StaDisconnected) {
                    self.state.update(InternalConnectionState::Disconnected);
                    self.controller_state = StaControllerState::ScanAndConnect;
                }
            }
            StaControllerState::Idle
            | StaControllerState::ScanAndConnect
            | StaControllerState::Connect(_) => {}
        }

        true
    }

    pub async fn handle_command(&mut self, command: Command, controller: &mut WifiController<'_>) {
        let (command, signal) = command;

        match command {
            StaCommand::ScanOnce => self.do_scan(controller).await,
        }

        signal.signal(());
    }

    pub async fn update(&mut self, controller: &mut WifiController<'_>) -> Duration {
        match self.controller_state {
            StaControllerState::Idle => NO_TIMEOUT,

            StaControllerState::ScanAndConnect => {
                self.do_scan(controller).await;
                self.controller_state = StaControllerState::Connect(CONNECT_RETRY_COUNT);
                CONTINUE
            }

            StaControllerState::Connect(retry) => {
                if retry == CONNECT_RETRY_COUNT {
                    match self.configure_for_visible_network(controller).await {
                        Ok(()) => {}
                        Err(NetworkConfigureError) => {
                            self.controller_state = StaControllerState::ScanAndConnect;
                            self.state.update(InternalConnectionState::NotConnected);
                            return SCAN_PERIOD;
                        }
                    }
                }

                match self.do_connect(controller).await {
                    Ok(_) => {
                        info!("Waiting to get IP address...");
                        self.controller_state = StaControllerState::AutoConnecting;
                        CONTINUE
                    }
                    Err(ConnectError) => {
                        if retry != 0 {
                            info!("Retrying...");
                            self.controller_state = StaControllerState::Connect(retry - 1);
                            return CONNECT_RETRY_PERIOD;
                        }

                        self.controller_state = StaControllerState::ScanAndConnect;
                        self.deprioritize_current().await;

                        SCAN_PERIOD
                    }
                }
            }

            StaControllerState::AutoConnecting => {
                let Some(config) = self.stack.config_v4() else {
                    return Duration::from_millis(500);
                };

                info!("Got IP: {}", config.address);
                self.state.update(InternalConnectionState::Connected);
                self.controller_state = StaControllerState::AutoConnected;
                CONTINUE
            }

            StaControllerState::AutoConnected => NO_TIMEOUT,
        }
    }

    pub(super) async fn wait_for_command(&self) -> Command {
        self.command_queue.receive().await
    }
}

#[cardio::task]
async fn sta_task(
    mut sta_controller: StaController,
    mut task_control: TaskControlToken<(), StaTaskResources>,
) {
    task_control
        .run_cancellable(|resources| async {
            sta_controller.setup(&mut resources.controller).await;

            info!("Starting wifi");
            unwrap!(resources.controller.start_async().await);
            info!("Wifi started!");

            loop {
                let events = sta_controller.events();

                let timeout = sta_controller.update(&mut resources.controller).await;

                let event_or_command = select(
                    async {
                        if timeout == NO_TIMEOUT {
                            Some(resources.controller.wait_for_events(events, false).await)
                        } else {
                            with_timeout(
                                timeout,
                                resources.controller.wait_for_events(events, false),
                            )
                            .await
                            .ok()
                        }
                    },
                    sta_controller.wait_for_command(),
                )
                .await;

                match event_or_command {
                    Either::First(Some(events)) => {
                        if !sta_controller.handle_events(events) {
                            return;
                        }
                    }
                    Either::Second(command) => {
                        sta_controller
                            .handle_command(command, &mut resources.controller)
                            .await;
                    }

                    _ => {}
                }
            }
        })
        .await;
}
