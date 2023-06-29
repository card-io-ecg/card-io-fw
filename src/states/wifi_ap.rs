use core::future::Future;

use bad_server::{
    handler::{RequestHandler, StaticHandler},
    BadServer,
};
use config_site::{
    data::{SharedWebContext, WebContext},
    handlers::{
        add_new_network::AddNewNetwork, delete_network::DeleteNetwork,
        list_known_networks::ListKnownNetworks, HEADER_FONT, INDEX_HANDLER,
    },
};
use embassy_executor::Spawner;
use embassy_futures::{
    join::join,
    select::{select, Either},
};
use embassy_net::{tcp::TcpSocket, Config, Ipv4Address, Ipv4Cidr, Stack, StaticConfig};
use embassy_sync::{blocking_mutex::raw::NoopRawMutex, signal::Signal};
use embassy_time::{Duration, Ticker, Timer};
use embedded_graphics::Drawable;
use embedded_svc::wifi::{AccessPointConfiguration, Configuration, Wifi};
use esp_wifi::wifi::{WifiController, WifiDevice, WifiEvent, WifiState};
use gui::screens::wifi_ap::{ApMenuEvents, WifiApScreen};

use crate::{
    board::{
        initialized::Board,
        wifi::driver::{as_static_mut, as_static_ref},
    },
    states::{WebserverResources, BIG_OBJECTS, MIN_FRAME_TIME, WEBSERVER_TASKS},
    AppState,
};

#[non_exhaustive]
pub struct Aborted {}

pub struct TaskController<R: Send> {
    token: Signal<NoopRawMutex, ()>,
    exited: Signal<NoopRawMutex, Result<R, Aborted>>,
}

impl<R: Send> TaskController<R> {
    pub const DEFAULT: Self = Self::new();

    pub const fn new() -> Self {
        Self {
            token: Signal::new(),
            exited: Signal::new(),
        }
    }

    pub async fn stop_from_outside(&self) -> Result<R, Aborted> {
        self.token.signal(());
        self.exited.wait().await
    }

    pub async fn run_cancellable(&self, future: impl Future<Output = R>) {
        let result = match select(future, self.token.wait()).await {
            Either::First(result) => Ok(result),
            Either::Second(_) => Err(Aborted {}),
        };
        self.exited.signal(result)
    }
}

pub async fn wifi_ap(board: &mut Board) -> AppState {
    board
        .wifi
        .initialize(&board.clocks, &mut board.peripheral_clock_control);

    let wifi_resources = unsafe { BIG_OBJECTS.as_wifi_ap_resources() };
    let (stack, controller) = board.wifi.configure_ap(
        Config::Static(StaticConfig {
            address: Ipv4Cidr::new(Ipv4Address::new(192, 168, 2, 1), 24),
            gateway: Some(Ipv4Address::from_bytes(&[192, 168, 2, 1])),
            dns_servers: Default::default(),
        }),
        &mut wifi_resources.stack_resources,
    );

    let spawner = Spawner::for_current_executor().await;

    let connection_task_control = TaskController::new();
    let net_task_control = TaskController::new();

    let webserver_task_control = [TaskController::DEFAULT; WEBSERVER_TASKS];

    let context = SharedWebContext::new(WebContext {
        known_networks: board.config.known_networks.clone(),
    });

    unsafe {
        spawner.must_spawn(connection_task(
            as_static_mut(controller),
            as_static_ref(&connection_task_control),
        ));
        spawner.must_spawn(net_task(
            as_static_ref(stack),
            as_static_ref(&net_task_control),
        ));

        #[allow(clippy::needless_range_loop)]
        for i in 0..WEBSERVER_TASKS {
            spawner.must_spawn(webserver_task(
                as_static_ref(stack),
                as_static_ref(&context),
                as_static_ref(&webserver_task_control[i]),
                as_static_mut(&mut wifi_resources.resources[i]),
            ));
        }
    }

    let mut screen = WifiApScreen::new(
        board.battery_monitor.battery_data().await,
        board.config.battery_style(),
    );

    let mut ticker = Ticker::every(MIN_FRAME_TIME);
    loop {
        let battery_data = board.battery_monitor.battery_data().await;

        if let Some(battery) = battery_data {
            if battery.is_low {
                // Enabling wifi modifies ADC readings and board shuts down
                #[cfg(feature = "battery_max17055")]
                break;
            }
        }

        screen.battery_data = battery_data;

        if let Some(event) = screen.menu.interact(board.frontend.is_touched()) {
            match event {
                ApMenuEvents::Exit => break,
            };
        }

        board
            .display
            .frame(|display| screen.draw(display))
            .await
            .unwrap();

        ticker.next().await;
    }

    for control in webserver_task_control {
        let _ = control.stop_from_outside().await;
    }

    let _ = join(
        connection_task_control.stop_from_outside(),
        net_task_control.stop_from_outside(),
    )
    .await;

    {
        let context = context.lock().await;
        if context.known_networks != board.config.known_networks {
            board.config.known_networks = context.known_networks.clone();
            board.config_changed = true;
            board.save_config().await;
        }
    }

    AppState::MainMenu
}

#[embassy_executor::task]
async fn connection_task(
    controller: &'static mut WifiController<'static>,
    task_control: &'static TaskController<()>,
) {
    task_control
        .run_cancellable(async {
            log::debug!("start connection task");
            log::debug!("Device capabilities: {:?}", controller.get_capabilities());

            loop {
                if let WifiState::ApStart = esp_wifi::wifi::get_wifi_state() {
                    // wait until we're no longer connected
                    controller.wait_for_event(WifiEvent::ApStop).await;
                    Timer::after(Duration::from_millis(5000)).await;

                    // TODO: exit app state if disconnected?
                }

                if !matches!(controller.is_started(), Ok(true)) {
                    let client_config = Configuration::AccessPoint(AccessPointConfiguration {
                        ssid: "Card/IO".into(),
                        ..Default::default()
                    });
                    controller.set_configuration(&client_config).unwrap();
                    log::debug!("Starting wifi");

                    controller.start().await.unwrap();
                    log::debug!("Wifi started!");
                }
            }
        })
        .await;
}

#[embassy_executor::task]
async fn net_task(
    stack: &'static Stack<WifiDevice<'static>>,
    task_control: &'static TaskController<!>,
) {
    task_control.run_cancellable(stack.run()).await;
}

#[embassy_executor::task(pool_size = super::WEBSERVER_TASKS)]
async fn webserver_task(
    stack: &'static Stack<WifiDevice<'static>>,
    context: &'static SharedWebContext,
    task_control: &'static TaskController<()>,
    buffers: &'static mut WebserverResources,
) {
    log::info!("Started webserver task");
    task_control
        .run_cancellable(async {
            while !stack.is_link_up() {
                Timer::after(Duration::from_millis(500)).await;
            }

            let mut socket = TcpSocket::new(stack, &mut buffers.rx_buffer, &mut buffers.tx_buffer);
            socket.set_timeout(Some(embassy_net::SmolDuration::from_secs(10)));

            BadServer::new()
                .with_request_buffer(&mut buffers.request_buffer[..])
                .with_header_count::<24>()
                .with_handler(RequestHandler::get("/", INDEX_HANDLER))
                .with_handler(RequestHandler::get("/font", HEADER_FONT))
                .with_handler(RequestHandler::get(
                    "/si",
                    StaticHandler::new(&[], env!("FW_VERSION").as_bytes()),
                ))
                .with_handler(RequestHandler::get("/kn", ListKnownNetworks { context }))
                .with_handler(RequestHandler::post("/nn", AddNewNetwork { context }))
                .with_handler(RequestHandler::post("/dn", DeleteNetwork { context }))
                .listen(&mut socket, 8080)
                .await;
        })
        .await;
    log::info!("Stopped webserver task");
}
