use core::{alloc::AllocError, future::pending, ptr::addr_of, sync::atomic::Ordering};

use crate::{
    board::{initialized::Context, wifi::net_task},
    task_control::{TaskControlToken, TaskController},
    Shared,
};
use alloc::{boxed::Box, rc::Rc, string::ToString, vec::Vec};
use config_site::data::network::WifiNetwork;
use embassy_executor::Spawner;
use embassy_futures::{
    join::join,
    select::{select, Either},
};
use embassy_net::{dns::DnsSocket, Runner, Stack};
use embassy_sync::{
    blocking_mutex::raw::NoopRawMutex,
    channel::Channel,
    mutex::{Mutex, MutexGuard},
    signal::Signal,
};
use embassy_time::{with_timeout, Duration, Timer};
use esp_hal::rng::Rng;
use esp_radio::wifi::{
    ap::AccessPointInfo, scan::ScanConfig, sta::StationConfig, Config, Interface, WifiController,
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
        })
    }

    async fn send_command(&self, command: StaCommand) -> bool {
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

    pub async fn scan(&self) {
        self.send_command(StaCommand::ScanOnce).await;
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
}

impl<'a> HttpsClientResources<'a> {
    pub fn client(&mut self) -> HttpClient<'_, TcpClient<'a>, DnsSocket<'a>> {
        let rng = Rng::new();
        let upper = rng.random() as u64;
        let lower = rng.random() as u64;
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
    connection_task_control: TaskController<(), StaTaskResources>,
    net_task_control: TaskController<()>,
    handle: Sta,
}

impl StaState {
    pub(super) fn init(
        controller: WifiController<'static>,
        sta_stack: Stack<'static>,
        sta_runner: Runner<'static, Interface<'static>>,
        spawner: Spawner,
    ) -> Self {
        info!("Starting STA");
        let networks = Rc::new(Mutex::new(heapless::Vec::new()));
        let known_networks = Rc::new(Mutex::new(Vec::new()));
        let state = Rc::new(StaConnectionState::new());
        let command_queue = Rc::new(CommandQueue::new());

        let connection_task_control =
            TaskController::from_resources(StaTaskResources { controller });
        let net_task_control = TaskController::new();

        info!("Starting STA tasks");
        spawner.spawn(unwrap!(sta_task(
            StaController::new(
                state.clone(),
                networks.clone(),
                known_networks.clone(),
                sta_stack.clone(),
                command_queue.clone(),
                InitialStaControllerState::ScanAndConnect,
            ),
            connection_task_control.token(),
        )));
        spawner.spawn(unwrap!(net_task(sta_runner, net_task_control.token())));

        Self {
            connection_task_control,
            net_task_control,
            handle: Sta {
                sta_stack,
                networks,
                known_networks,
                state,
                command_queue,
            },
        }
    }

    pub(super) async fn stop(self) {
        info!("Stopping STA");
        let _ = join(
            self.connection_task_control.stop(),
            self.net_task_control.stop(),
        )
        .await;

        info!("Stopped STA");
    }

    pub(crate) fn handle(&self) -> &Sta {
        &self.handle
    }
}

struct StaTaskResources {
    controller: WifiController<'static>,
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

pub enum StaControllerState {
    Idle,
    ScanAndConnect,
    Connect(u8),    // select network, start connection
    AutoConnecting, // waiting for IP
    AutoConnected,  // wait for disconnection
}

impl StaControllerState {
    pub fn is_connected(&self) -> bool {
        matches!(self, Self::AutoConnected)
    }
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
    pub(crate) controller_state: StaControllerState,

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

        let client_config = Config::Station(StationConfig::default());
        unwrap!(controller.set_config(&client_config));
    }

    async fn do_scan(&mut self, controller: &mut WifiController<'_>) {
        info!("Scanning...");
        let mut scan_results = controller
            .scan_async(&ScanConfig::default().with_max(SCAN_RESULTS))
            .await;

        match scan_results.as_mut() {
            Ok(ref mut visible_networks) => {
                info!("Found {} access points", visible_networks.len());

                // Sort by signal strength, descending
                visible_networks.sort_by(|a, b| b.signal_strength.cmp(&a.signal_strength));

                let mut networks = self.networks.lock().await;

                networks.clear();
                if networks
                    .extend_from_slice(
                        &visible_networks[0..SCAN_RESULTS.min(visible_networks.len())],
                    )
                    .is_err()
                {
                    error!(
                        "Failed to store {} visible networks",
                        visible_networks.len()
                    );
                }
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
                    .find(|(kn, pref)| kn.ssid == network.ssid.as_str() && *pref == preference)
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

        unwrap!(controller.set_config(&Config::Station(
            StationConfig::default()
                .with_ssid(connect_to.ssid.as_str().to_string())
                .with_password(connect_to.pass.as_str().to_string())
        )));

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

    pub(super) fn on_disconnected(&mut self) {
        self.state.update(InternalConnectionState::Disconnected);
        self.controller_state = StaControllerState::ScanAndConnect;
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

            loop {
                let timeout = sta_controller.update(&mut resources.controller).await;

                let poll_result = select(
                    async {
                        if sta_controller.controller_state.is_connected() {
                            _ = resources.controller.wait_for_disconnect_async().await;
                            true
                        } else if timeout == NO_TIMEOUT {
                            pending().await
                        } else {
                            Timer::after(timeout).await;
                            false
                        }
                    },
                    sta_controller.wait_for_command(),
                )
                .await;

                match poll_result {
                    Either::First(disconnected) if disconnected => {
                        sta_controller.on_disconnected();
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
