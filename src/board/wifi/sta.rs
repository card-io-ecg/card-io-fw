use core::{alloc::AllocError, cell::RefCell, future::pending};

use crate::{
    board::{initialized::Context, wifi::net_task},
    task_control::{TaskControlToken, TaskController},
    Shared,
};
use alloc::{rc::Rc, vec::Vec};
use config_site::data::network::{StationStatus, WifiNetwork};
use embassy_executor::Spawner;
use embassy_futures::{
    join::join,
    select::{select, select3, Either3},
};
use embassy_net::{Runner, Stack};
use embassy_sync::{
    blocking_mutex::raw::NoopRawMutex,
    channel::Channel,
    mutex::{Mutex, MutexGuard},
    signal::Signal,
};
use embassy_time::{with_timeout, Duration, Instant, Timer};
use esp_hal::rng::Rng;
use esp_radio::wifi::{
    ap::{AccessPointConfig, AccessPointInfo},
    scan::ScanConfig,
    sta::StationConfig,
    AuthenticationMethodConfig, Config, Interface, WifiController,
};
use gui::widgets::wifi_client::WifiClientState;
use heapless::String;
use macros as cardio;
use network_services::{
    client::{Client, ClientError, Connection},
    url::BaseUrl,
};

pub(super) const SCAN_RESULTS: usize = 20;

pub(super) struct StaConnectionState {
    signal: Signal<NoopRawMutex, ()>,
    value: RefCell<StationStatus>,
}

impl StaConnectionState {
    pub fn new() -> StaConnectionState {
        Self {
            signal: Signal::new(),
            value: RefCell::new(StationStatus::Disconnected),
        }
    }

    async fn wait(&self) -> WifiClientState {
        self.signal.wait().await;
        self.client_state()
    }

    fn client_state(&self) -> WifiClientState {
        match *self.value.borrow() {
            StationStatus::Disconnected | StationStatus::Failed(_) => WifiClientState::NotConnected,
            StationStatus::Joining(_) => WifiClientState::Connecting,
            StationStatus::Joined(_) => WifiClientState::Connected,
        }
    }

    fn status(&self) -> StationStatus {
        self.value.borrow().clone()
    }

    fn ssid(&self) -> Option<String<32>> {
        match &*self.value.borrow() {
            StationStatus::Joining(ssid)
            | StationStatus::Joined(ssid)
            | StationStatus::Failed(ssid) => Some(ssid.clone()),
            StationStatus::Disconnected => None,
        }
    }

    /// Moves the network of the current join attempt to `status`.
    fn advance(&self, status: fn(String<32>) -> StationStatus) {
        self.update(status(unwrap!(self.ssid())));
    }

    fn is_linked_to(&self, ssid: &str) -> bool {
        matches!(
            &*self.value.borrow(),
            StationStatus::Joining(linked) | StationStatus::Joined(linked) if linked == ssid
        )
    }

    fn update(&self, value: StationStatus) {
        debug!("Updating connection state: {:?}", value);
        *self.value.borrow_mut() = value;
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

#[derive(Clone)]
pub struct Sta {
    pub(super) sta_stack: Stack<'static>,
    pub(super) networks: Shared<heapless::Vec<AccessPointInfo, SCAN_RESULTS>>,
    pub(super) known_networks: Shared<Vec<KnownNetwork>>,
    pub(super) state: Rc<StaConnectionState>,
    pub(super) command_queue: Rc<CommandQueue>,
    pub(super) networks_changed: Rc<Signal<NoopRawMutex, ()>>,
}

impl Sta {
    pub fn connection_state(&self) -> WifiClientState {
        self.state.client_state()
    }

    pub fn status(&self) -> StationStatus {
        self.state.status()
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
        self.networks_changed.signal(());
    }

    pub async fn wait_for_state_change(&self) -> WifiClientState {
        self.state.wait().await
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

    pub fn client(&self) -> Result<StaClient, AllocError> {
        Ok(StaClient {
            client: Client::new()?,
            stack: self.sta_stack,
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

    pub async fn drop_link(&self, ssid: &str) {
        let Ok(ssid) = String::try_from(ssid) else {
            return;
        };
        self.send_command(StaCommand::DropLink(ssid)).await;
    }
}

pub struct StaClient {
    pub client: Client,
    pub stack: Stack<'static>,
}

impl StaClient {
    pub async fn connect<'c>(
        &'c mut self,
        base: &BaseUrl<'_>,
    ) -> Result<Connection<'c>, ClientError> {
        self.client
            .connect(self.stack, WifiRng::default(), base)
            .await
    }
}

/// The hardware RNG mixes in RF noise while Wi-Fi runs, which makes it a true random source
/// (esp-hal `rng` docs). The client runs only with Wi-Fi up.
#[derive(Default)]
pub struct WifiRng(Rng);

impl rand_core::RngCore for WifiRng {
    fn next_u32(&mut self) -> u32 {
        self.0.next_u32()
    }

    fn next_u64(&mut self) -> u64 {
        self.0.next_u64()
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        self.0.fill_bytes(dest);
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
        self.0.try_fill_bytes(dest)
    }
}

impl rand_core::CryptoRng for WifiRng {}

pub(super) struct StaState {
    connection_task_control: TaskController<(), StaTaskResources>,
    net_task_control: TaskController<()>,
    handle: Sta,
}

impl StaState {
    pub(super) fn init(
        controller: WifiController<'static>,
        sta_stack: Stack<'static>,
        sta_runner: Runner<'static, Interface>,
        spawner: Spawner,
    ) -> Self {
        info!("Starting STA");
        let networks = Rc::new(Mutex::new(heapless::Vec::new()));
        let known_networks = Rc::new(Mutex::new(Vec::new()));
        let state = Rc::new(StaConnectionState::new());
        let command_queue = Rc::new(CommandQueue::new());
        let networks_changed = Rc::new(Signal::new());

        let connection_task_control =
            TaskController::from_resources(StaTaskResources { controller });
        let net_task_control = TaskController::new();

        info!("Starting STA tasks");
        spawner.spawn(unwrap!(sta_task(
            StaController::new(
                state.clone(),
                networks.clone(),
                known_networks.clone(),
                sta_stack,
                command_queue.clone(),
                networks_changed.clone(),
                None,
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
                networks_changed,
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

pub enum StaControllerState {
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

const SCAN_PERIOD: Duration = Duration::from_secs(5);
const CONTINUE: Duration = Duration::from_millis(0);
const CONNECT_RETRY_PERIOD: Duration = Duration::from_millis(100);
const CONNECT_RETRY_COUNT: u8 = 5;

pub enum StaCommand {
    ScanOnce,
    DropLink(String<32>),
}

struct ConnectError;
struct NetworkConfigureError;

pub(super) struct StaController {
    state: Rc<StaConnectionState>,
    pub(crate) controller_state: StaControllerState,

    networks: Shared<heapless::Vec<AccessPointInfo, SCAN_RESULTS>>,
    known_networks: Shared<Vec<KnownNetwork>>,
    stack: Stack<'static>,
    access_point: Option<AccessPointConfig>,

    /// When `step` runs next. `None` means it waits for an event: a disconnect or a new network.
    next_step: Option<Instant>,

    command_queue: Rc<CommandQueue>,
    networks_changed: Rc<Signal<NoopRawMutex, ()>>,
}

impl StaController {
    pub fn new(
        state: Rc<StaConnectionState>,
        networks: Shared<heapless::Vec<AccessPointInfo, SCAN_RESULTS>>,
        known_networks: Shared<Vec<KnownNetwork>>,
        stack: Stack<'static>,
        command_queue: Rc<CommandQueue>,
        networks_changed: Rc<Signal<NoopRawMutex, ()>>,
        access_point: Option<AccessPointConfig>,
    ) -> Self {
        Self {
            state,
            networks,
            known_networks,
            stack,
            command_queue,
            networks_changed,
            access_point,
            next_step: Some(Instant::now()),
            controller_state: StaControllerState::ScanAndConnect,
        }
    }

    /// A station-only configuration switches the Wi-Fi mode and stops the access point.
    fn config(&self, station: StationConfig) -> Config {
        match &self.access_point {
            Some(access_point) => Config::AccessPointStation(station, access_point.clone()),
            None => Config::Station(station),
        }
    }

    pub(super) async fn setup(&mut self, controller: &mut WifiController<'_>) {
        info!("Configuring STA");

        let client_config = self.config(StationConfig::default());
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
        self.state
            .update(StationStatus::Joining(connect_to.ssid.clone()));

        let station_config = StationConfig::default()
            .with_ssid(unwrap!(connect_to.ssid.as_str().try_into()))
            .with_authentication(AuthenticationMethodConfig::Wpa2Personal(unwrap!(
                connect_to.pass.as_str().try_into()
            )));
        unwrap!(controller.set_config(&self.config(station_config)));

        Ok(())
    }

    async fn do_connect(
        &mut self,
        controller: &mut WifiController<'_>,
    ) -> Result<(), ConnectError> {
        self.state.advance(StationStatus::Joining);
        match with_timeout(Duration::from_secs(30), controller.connect_async()).await {
            Ok(Ok(_)) => {
                self.state.advance(StationStatus::Joining);
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
        if let Some(ssid) = self.state.ssid() {
            let mut known_networks = self.known_networks.lock().await;
            if let Some((_, preference)) = known_networks.iter_mut().find(|(kn, preference)| {
                kn.ssid == ssid && *preference == NetworkPreference::Preferred
            }) {
                *preference = NetworkPreference::Deprioritized;
            }
        }
    }

    pub(super) fn on_disconnected(&mut self) {
        self.state.update(StationStatus::Disconnected);
        self.controller_state = StaControllerState::ScanAndConnect;
        self.next_step = Some(Instant::now());
    }

    /// The link may be down already, for example between two join retries. The state resets
    /// either way, so the next search does not wait for an event that never comes.
    async fn drop_link(&mut self, ssid: &str, controller: &mut WifiController<'_>) {
        if !self.state.is_linked_to(ssid) {
            return;
        }

        info!("Dropping the link to {}", ssid);
        if let Err(e) = controller.disconnect_async().await {
            debug!("Disconnect failed: {:?}", e);
        }
        self.on_disconnected();
    }

    /// The station idles in `ScanAndConnect` while it has no network to join. The new list
    /// ends that without waiting for a `SCAN_PERIOD`.
    pub(super) fn on_networks_changed(&mut self) {
        if self.next_step.is_none()
            && matches!(self.controller_state, StaControllerState::ScanAndConnect)
        {
            self.next_step = Some(Instant::now());
        }
    }

    pub async fn handle_command(&mut self, command: Command, controller: &mut WifiController<'_>) {
        let (command, signal) = command;

        match command {
            StaCommand::ScanOnce => self.do_scan(controller).await,
            StaCommand::DropLink(ssid) => self.drop_link(&ssid, controller).await,
        }

        signal.signal(());
    }

    /// Runs `step` when it is due and returns when the next one is due. Other wake-ups, like an
    /// access point event or a page scan, leave the deadline alone, so search passes stay at
    /// least `SCAN_PERIOD` apart.
    pub async fn update(&mut self, controller: &mut WifiController<'_>) -> Option<Instant> {
        if self.next_step.is_some_and(|due| due <= Instant::now()) {
            let wait = self.step(controller).await;
            self.next_step = wait.map(|wait| Instant::now() + wait);
        }

        self.next_step
    }

    async fn step(&mut self, controller: &mut WifiController<'_>) -> Option<Duration> {
        match self.controller_state {
            StaControllerState::ScanAndConnect => {
                if self.known_networks.lock().await.is_empty() {
                    return None;
                }

                self.do_scan(controller).await;
                self.controller_state = StaControllerState::Connect(CONNECT_RETRY_COUNT);
                Some(CONTINUE)
            }

            StaControllerState::Connect(retry) => {
                if retry == CONNECT_RETRY_COUNT {
                    match self.configure_for_visible_network(controller).await {
                        Ok(()) => {}
                        Err(NetworkConfigureError) => {
                            self.controller_state = StaControllerState::ScanAndConnect;
                            self.state.update(StationStatus::Disconnected);
                            return Some(SCAN_PERIOD);
                        }
                    }
                }

                match self.do_connect(controller).await {
                    Ok(_) => {
                        info!("Waiting to get IP address...");
                        self.controller_state = StaControllerState::AutoConnecting;
                        Some(CONTINUE)
                    }
                    Err(ConnectError) => {
                        if retry != 0 {
                            info!("Retrying...");
                            self.controller_state = StaControllerState::Connect(retry - 1);
                            return Some(CONNECT_RETRY_PERIOD);
                        }

                        self.controller_state = StaControllerState::ScanAndConnect;
                        self.state.advance(StationStatus::Failed);
                        self.deprioritize_current().await;

                        Some(SCAN_PERIOD)
                    }
                }
            }

            StaControllerState::AutoConnecting => {
                let Some(config) = self.stack.config_v4() else {
                    return Some(Duration::from_millis(500));
                };

                info!("Got IP: {}", config.address);
                self.state.advance(StationStatus::Joined);
                self.controller_state = StaControllerState::AutoConnected;
                Some(CONTINUE)
            }

            StaControllerState::AutoConnected => None,
        }
    }

    pub(super) async fn wait_for_command(&self) -> Command {
        self.command_queue.receive().await
    }

    pub(super) async fn wait_for_networks_changed(&self) {
        self.networks_changed.wait().await;
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
                let next_step = sta_controller.update(&mut resources.controller).await;

                let poll_result = select3(
                    async {
                        if sta_controller.controller_state.is_connected() {
                            _ = resources.controller.wait_for_disconnect_async().await;
                            true
                        } else if let Some(next_step) = next_step {
                            Timer::at(next_step).await;
                            false
                        } else {
                            pending().await
                        }
                    },
                    sta_controller.wait_for_command(),
                    sta_controller.wait_for_networks_changed(),
                )
                .await;

                match poll_result {
                    Either3::First(disconnected) if disconnected => {
                        sta_controller.on_disconnected();
                    }
                    Either3::Second(command) => {
                        sta_controller
                            .handle_command(command, &mut resources.controller)
                            .await;
                    }
                    Either3::Third(()) => sta_controller.on_networks_changed(),

                    _ => {}
                }
            }
        })
        .await;
}
