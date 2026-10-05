mod session;

use core::{
    cell::Cell,
    net::{IpAddr, Ipv4Addr, SocketAddr},
};

use alloc::{boxed::Box, rc::Rc};
use config_site::{
    data::{
        network::{StationStatus, VisibleNetwork, WifiNetwork, MAX_VISIBLE_NETWORKS},
        SharedWebContext, WebContext,
    },
    ConfigSite, PairingControl, Station,
};
use edge_http::io::server::Server;
use edge_nal::{TcpBind, WithTimeout};
use edge_nal_embassy::{Tcp, TcpBuffers};
use embassy_executor::Spawner;
use embassy_futures::select::{select3, Either3};
use embassy_sync::{
    blocking_mutex::raw::NoopRawMutex, channel::Channel, mutex::Mutex, signal::Signal,
};
use embassy_time::{Duration, Ticker, Timer};
use embedded_graphics::Drawable;
use esp_radio::wifi::AuthenticationMethod;
use gui::{
    screens::wifi_ap::{ApMenuEvents, ApRequest, WifiApScreen},
    widgets::{wifi_access_point::WifiAccessPointState, wifi_client::WifiClientState},
};
use macros as cardio;
use network_services::{
    pairing::{
        self, parse_code, Code, Job, Name, Outcome, PairAnswer, Pairing, Refusal, SigningKey,
        Status, UnpairAnswer,
    },
    url,
};

use crate::{
    board::{
        initialized::{Context, Save},
        wifi::{ap::Ap, sta::Sta},
    },
    states::{
        menu::{
            wifi_ap::session::{new_key, session_task, Request, ScanLock, Session},
            AppMenu,
        },
        TouchInputShaper, MENU_FRAME_TIME, MENU_IDLE_DURATION, WEBSERVER_TASKS,
    },
    task_control::{TaskControlToken, TaskController},
    timeout::Timeout,
    AppState, SerialNumber,
};

enum Command {
    Pair(Code),
    Unpair,
}

type Reply = Rc<Signal<NoopRawMutex, Result<(), Refusal>>>;
type Commands = Channel<NoopRawMutex, (Command, Reply), 1>;

pub async fn wifi_ap(context: &mut Context) -> AppState {
    let Some((ap, sta)) = context.enable_wifi_ap_sta().await else {
        // FIXME: Show error screen
        return AppState::Menu(AppMenu::Main);
    };

    let spawner = unsafe { Spawner::for_current_executor().await };

    context.pairing.session_opened();
    let name = Rc::new(Name::from_mac(SerialNumber::bytes()));
    let status = Rc::new(Cell::new(context.pairing.status()));
    let commands = Rc::new(Commands::new());
    let session = Rc::new(Session::new());
    let scan_lock = Rc::new(ScanLock::new(()));

    let web_context = Rc::new(SharedWebContext::new(WebContext::new(
        context.config.known_networks.clone(),
        context.config.backend_url.clone(),
    )));
    let station = Rc::new(SetupStation {
        sta: sta.clone(),
        scan_lock: scan_lock.clone(),
        web_context: web_context.clone(),
        networks: Mutex::new(context.config.known_networks.clone()),
    });

    let webserver_task_control = TaskController::new();
    spawner.spawn(unwrap!(webserver_task(
        ap.clone(),
        web_context.clone(),
        station.clone(),
        SetupPairing {
            name: name.clone(),
            status: status.clone(),
            commands: commands.clone(),
        },
        webserver_task_control.token(),
    )));

    let session_task_control = TaskController::new();
    spawner.spawn(unwrap!(session_task(
        sta.clone(),
        name,
        scan_lock,
        context.counters.clone(),
        session.clone(),
        session_task_control.token(),
    )));

    let mut screen = WifiApScreen::new();

    let mut ticker = Ticker::every(MENU_FRAME_TIME);
    let mut exit_timer = Timeout::new(MENU_IDLE_DURATION);
    let mut input = TouchInputShaper::new();

    let mut prev_timeout = 0;
    let mut closing = false;

    loop {
        input.update(&mut context.frontend);
        let is_touched = input.is_touched();

        // We only enable this check for fuel gauges because enabling wifi modifies ADC readings
        // and the board would shut down immediately.
        closing |= context.battery_monitor.is_low();

        let request = running_request(&context.pairing);
        let timeout = exit_timer.remaining().as_secs() as u8;
        let connection_state: WifiAccessPointState = ap.connection_state().into();
        if connection_state != WifiAccessPointState::Connected {
            // We start counting when the last client disconnects, and we reset on interaction.
            // The timer does not run out while a request runs.
            if screen.state == WifiAccessPointState::Connected || is_touched || request.is_some() {
                exit_timer.reset();
            }

            closing |= exit_timer.is_elapsed();
            screen.timeout = Some(timeout);
        } else {
            screen.timeout = None;
        }

        let changed = connection_state != screen.state
            || prev_timeout != timeout
            || request != screen.request;

        prev_timeout = timeout;
        screen.state = connection_state;
        screen.request = request;

        #[allow(irrefutable_let_patterns)]
        if let Some(ApMenuEvents::Exit) = screen.menu.interact(is_touched) {
            closing = true;
        }

        // Closing waits for the answer of a running request.
        if closing && request.is_none() {
            break;
        }

        context
            .with_status_bar(|display| {
                if screen.menu.update(display) || changed {
                    screen.draw(display).map(|_| true)
                } else {
                    Ok(false)
                }
            })
            .await;

        station.sync_networks().await;

        match select3(
            ticker.next(),
            commands.receive(),
            session.outcomes.receive(),
        )
        .await
        {
            Either3::First(()) => {}
            Either3::Second((command, reply)) => {
                let result = start_request(context, &sta, &web_context, &session, command).await;
                status.set(context.pairing.status());
                reply.signal(result);
            }
            Either3::Third(outcome) => {
                finish_request(context, outcome).await;
                status.set(context.pairing.status());
            }
        }
    }

    let _ = webserver_task_control.stop().await;
    let _ = session_task_control.stop().await;

    context.disable_wifi().await;

    apply_page_settings(context, &web_context).await;
    if context.save_config().await == Save::Written {
        context.pairing.saved();
    }

    AppState::Menu(AppMenu::Main)
}

fn running_request(pairing: &Pairing) -> Option<ApRequest> {
    match pairing.status() {
        Status::Pairing => Some(ApRequest::Pairing),
        Status::Unpairing => Some(ApRequest::Unpairing),
        Status::Unpaired(_) | Status::Paired(_) => None,
    }
}

async fn apply_page_settings(context: &mut Context, web_context: &SharedWebContext) {
    let web_context = web_context.lock().await;
    context.update_config(|config| {
        if web_context.known_networks != config.known_networks {
            config
                .known_networks
                .clone_from(&web_context.known_networks);
        }
        if web_context.backend_url != config.backend_url {
            config.backend_url.clone_from(&web_context.backend_url);
        }
    });
}

/// Checks a Pair or Unpair press in the order of the page's refusal words, then starts it.
async fn start_request(
    context: &mut Context,
    sta: &Sta,
    web_context: &SharedWebContext,
    session: &Session,
    command: Command,
) -> Result<(), Refusal> {
    if sta.connection_state() != WifiClientState::Connected {
        return Err(Refusal::Wifi);
    }
    if running_request(&context.pairing).is_some() {
        return Err(Refusal::Busy);
    }

    apply_page_settings(context, web_context).await;
    if context.storage.is_none() || context.save_config().await == Save::Failed {
        return Err(Refusal::Save);
    }

    let url = context.config.backend_url.clone();
    if url::parse(&url).is_none() {
        return Err(Refusal::Url);
    }

    let job = match command {
        Command::Pair(code) => {
            context.pairing.start_pair()?;
            info!("Registering the device");
            Job::Register(code, new_key())
        }
        Command::Unpair => {
            let key = context.pairing.start_unpair()?;
            info!("Removing the device");
            Job::Unpair(key)
        }
    };
    session.requests.send(Request { url, job }).await;
    Ok(())
}

async fn finish_request(context: &mut Context, outcome: Outcome) {
    match outcome {
        Outcome::Registered(answer) => {
            match &answer {
                PairAnswer::Registered(key) => {
                    info!("The device is registered");
                    store_key(context, key).await;
                }
                PairAnswer::Failed(failure) => warn!("Registration failed: {:?}", failure),
            }
            context.pairing.pair_answered(answer);
        }
        Outcome::Unpaired(answer) => {
            info!("Unpair result: {:?}", answer);
            if answer == UnpairAnswer::Removed {
                forget_key(context).await;
            }
            context.pairing.unpair_answered(answer);
        }
    }
}

async fn store_key(context: &mut Context, key: &SigningKey) {
    let Some(storage) = context.storage.as_mut() else {
        warn!("Storage unavailable. A reboot forgets the pairing");
        return;
    };
    if let Err(e) = storage.save_key(key).await {
        error!("Failed to save the device key: {:?}", e);
    }
}

async fn forget_key(context: &mut Context) {
    let Some(storage) = context.storage.as_mut() else {
        warn!("Storage unavailable. A reboot loads the old device key");
        return;
    };
    if let Err(e) = storage.delete_key().await {
        error!("Failed to delete the device key: {:?}", e);
    }
}

const WEBSERVER_PORT: u16 = 80;
const SOCKET_TIMEOUT_MS: u32 = 10_000;
const KEEPALIVE_TIMEOUT_MS: u32 = 5_000;

struct WebserverResources {
    buffers: TcpBuffers<WEBSERVER_TASKS, 4096, 4096>,
    server: Server<WEBSERVER_TASKS, 2048, 24>,
}

#[cardio::task]
async fn webserver_task(
    ap: Ap,
    context: Rc<SharedWebContext>,
    station: Rc<SetupStation>,
    pairing: SetupPairing,
    mut task_control: TaskControlToken<()>,
) {
    info!("Started webserver task");
    task_control
        .run_cancellable(|_| async {
            while !ap.is_active() {
                Timer::after(Duration::from_millis(500)).await;
            }

            let mut resources = Box::new(WebserverResources {
                buffers: TcpBuffers::new(),
                server: Server::new(),
            });

            let tcp = Tcp::new(ap.stack(), &resources.buffers);
            let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), WEBSERVER_PORT);
            let acceptor = match tcp.bind(address).await {
                Ok(acceptor) => WithTimeout::new(SOCKET_TIMEOUT_MS, acceptor),
                Err(e) => {
                    warn!("Failed to bind webserver socket: {:?}", e);
                    return;
                }
            };

            let handler = ConfigSite::new(&context, &*station, &pairing, env!("FW_VERSION"));

            if let Err(e) = resources
                .server
                .run(Some(KEEPALIVE_TIMEOUT_MS), acceptor, handler)
                .await
            {
                warn!("Webserver error: {:?}", defmt::Debug2Format(&e));
            }
        })
        .await;
    info!("Stopped webserver task");
}

/// Reaches the setup loop, which owns the pairing state, through its command channel.
struct SetupPairing {
    name: Rc<Name>,
    status: Rc<Cell<Status>>,
    commands: Rc<Commands>,
}

impl SetupPairing {
    async fn ask(&self, command: Command) -> Result<(), Refusal> {
        let reply = Rc::new(Signal::new());
        self.commands.send((command, reply.clone())).await;
        reply.wait().await
    }
}

impl PairingControl for SetupPairing {
    async fn status_line(&self, out: &mut impl core::fmt::Write) {
        unwrap!(pairing::format(&self.name, &self.status.get(), out));
    }

    async fn pair(&self, typed: &str) -> Result<(), Refusal> {
        let code = parse_code(typed).ok_or(Refusal::Code)?;
        self.ask(Command::Pair(code)).await
    }

    async fn unpair(&self) -> Result<(), Refusal> {
        self.ask(Command::Unpair).await
    }
}

/// The station as the setup page sees it. `networks` is the list the station holds, so the
/// setup loop and `drop_link` give it the page's list once, whichever runs first.
struct SetupStation {
    sta: Sta,
    scan_lock: Rc<ScanLock>,
    web_context: Rc<SharedWebContext>,
    networks: Mutex<NoopRawMutex, heapless::Vec<WifiNetwork, 8>>,
}

impl SetupStation {
    /// Gives the station the networks the page lists. Replacing the list resets the preference of
    /// each network, so it runs only when the list changed. The lock on `networks` is held until
    /// the station has the list, so a second caller returns only after the first one is done.
    async fn sync_networks(&self) {
        let mut networks = self.networks.lock().await;
        {
            let web_context = self.web_context.lock().await;
            if web_context.known_networks == *networks {
                return;
            }
            networks.clone_from(&web_context.known_networks);
        }

        self.sta.update_known_networks(&networks).await;
    }
}

impl Station for SetupStation {
    async fn status(&self) -> StationStatus {
        self.sta.status()
    }

    async fn visible_networks(
        &self,
        out: &mut heapless::Vec<VisibleNetwork, MAX_VISIBLE_NETWORKS>,
    ) {
        let visible_networks = self.sta.visible_networks().await;
        let visible = visible_networks
            .iter()
            .filter(|network| !network.ssid.as_str().is_empty())
            .filter_map(|network| {
                Some(VisibleNetwork {
                    ssid: heapless::String::try_from(network.ssid.as_str()).ok()?,
                    rssi: network.signal_strength,
                    locked: network.auth_method != Some(AuthenticationMethod::None),
                })
            });

        for network in visible {
            if out.push(network).is_err() {
                break;
            }
        }
    }

    async fn scan(&self) {
        let Ok(_scan) = self.scan_lock.try_lock() else {
            return;
        };
        self.sta.scan().await;
    }

    // The page has removed the network from its list before it calls this. The station gets that
    // list first, or its next search could join the forgotten network again.
    async fn drop_link(&self, ssid: &str) {
        self.sync_networks().await;
        self.sta.drop_link(ssid).await;
    }
}
