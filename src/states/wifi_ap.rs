use core::future::Future;

use bad_server::{
    connector::Connection,
    handler::{RequestHandler, StaticHandler},
    request::Request,
    response::ResponseStatus,
    BadServer, HandleError, Header,
};
use config_site::{HEADER_FONT, INDEX_HANDLER};
use embassy_executor::Spawner;
use embassy_futures::{join::join, select::select};
use embassy_net::{
    tcp::TcpSocket, Config, Ipv4Address, Ipv4Cidr, Stack, StackResources, StaticConfig,
};
use embassy_sync::{blocking_mutex::raw::NoopRawMutex, mutex::Mutex, signal::Signal};
use embassy_time::{Duration, Ticker, Timer};
use embedded_graphics::Drawable;
use embedded_svc::wifi::{AccessPointConfiguration, Configuration, Wifi};
use esp_wifi::wifi::{WifiController, WifiDevice, WifiEvent, WifiMode, WifiState};
use gui::screens::wifi_ap::{ApMenuEvents, WifiApScreen};

use crate::{board::initialized::Board, states::MIN_FRAME_TIME, AppState};

unsafe fn as_static_ref<T>(what: &T) -> &'static T {
    core::mem::transmute(what)
}

unsafe fn as_static_mut<T>(what: &mut T) -> &'static mut T {
    core::mem::transmute(what)
}

pub struct WebContext {}
pub type SharedWebContext = Mutex<NoopRawMutex, WebContext>;

struct TaskController {
    token: Signal<NoopRawMutex, ()>,
    exited: Signal<NoopRawMutex, ()>,
}

impl TaskController {
    fn new() -> Self {
        Self {
            token: Signal::new(),
            exited: Signal::new(),
        }
    }

    async fn stop_from_outside(&self) {
        self.token.signal(());
        self.exited.wait().await;
    }

    async fn run_cancellable(&self, future: impl Future) {
        select(future, self.token.wait()).await;
        self.exited.signal(())
    }
}

pub async fn wifi_ap(board: &mut Board) -> AppState {
    let (wifi, init) = board
        .wifi
        .driver_mut(&board.clocks, &mut board.peripheral_clock_control);
    let (wifi_interface, controller) = esp_wifi::wifi::new_with_mode(
        unsafe { as_static_mut(init) },
        unsafe { as_static_mut(wifi) },
        WifiMode::Ap,
    );

    let config = Config::Static(StaticConfig {
        address: Ipv4Cidr::new(Ipv4Address::new(192, 168, 2, 1), 24),
        gateway: Some(Ipv4Address::from_bytes(&[192, 168, 2, 1])),
        dns_servers: Default::default(),
    });
    let mut stack_resources = StackResources::<3>::new();
    let stack = Stack::new(
        wifi_interface,
        config,
        unsafe { as_static_mut(&mut stack_resources) },
        1234,
    );

    let spawner = Spawner::for_current_executor().await;

    let connection_task_control = TaskController::new();
    let net_task_control = TaskController::new();
    let webserver_task_control = TaskController::new();
    let webserver_task_control2 = TaskController::new();

    let context = SharedWebContext::new(WebContext {});

    unsafe {
        spawner.must_spawn(connection_task(
            controller,
            as_static_ref(&connection_task_control),
        ));
        spawner.must_spawn(net_task(
            as_static_ref(&stack),
            as_static_ref(&net_task_control),
        ));
        spawner.must_spawn(webserver_task(
            as_static_ref(&stack),
            as_static_ref(&context),
            as_static_ref(&webserver_task_control),
        ));
        spawner.must_spawn(webserver_task(
            as_static_ref(&stack),
            as_static_ref(&context),
            as_static_ref(&webserver_task_control2),
        ));
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
                // return AppState::Shutdown;
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

    webserver_task_control.stop_from_outside().await;
    webserver_task_control2.stop_from_outside().await;

    join(
        connection_task_control.stop_from_outside(),
        net_task_control.stop_from_outside(),
    )
    .await;

    AppState::MainMenu
}

#[embassy_executor::task]
async fn connection_task(
    mut controller: WifiController<'static>,
    task_control: &'static TaskController,
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
    task_control: &'static TaskController,
) {
    task_control.run_cancellable(stack.run()).await;
}

struct DemoHandler;
impl<C: Connection> RequestHandler<C> for DemoHandler {
    async fn handle(&self, request: Request<'_, '_, C>) -> Result<(), HandleError<C>> {
        let mut response = request.send_response(ResponseStatus::Ok).await?;
        response
            .send_header(Header {
                name: "Content-Length",
                value: b"13",
            })
            .await?;
        let mut response = response.start_body().await?;
        response.write_string("Hello, world!").await?;
        Ok(())
    }
}

#[embassy_executor::task(pool_size = 2)]
async fn webserver_task(
    stack: &'static Stack<WifiDevice<'static>>,
    _context: &'static SharedWebContext,
    task_control: &'static TaskController,
) {
    task_control
        .run_cancellable(async {
            while !stack.is_link_up() {
                Timer::after(Duration::from_millis(500)).await;
            }

            let mut rx_buffer = [0; 4096];
            let mut tx_buffer = [0; 4096];
            let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
            socket.set_timeout(Some(embassy_net::SmolDuration::from_secs(10)));

            BadServer::new()
                .with_request_buffer_size::<2048>()
                .with_header_count::<48>()
                .with_handler(RequestHandler::get("/", INDEX_HANDLER))
                .with_handler(RequestHandler::get("/font", HEADER_FONT))
                .with_handler(RequestHandler::get("/demo", DemoHandler))
                .with_handler(RequestHandler::get(
                    "/si",
                    StaticHandler::new(&[], env!("FW_VERSION").as_bytes()),
                ))
                .with_handler(RequestHandler::get(
                    "/kn",
                    StaticHandler::new(&[], b"Network1\nNetwork2\nNetwork3"),
                ))
                .listen(&mut socket, 8080)
                .await;
        })
        .await;
}
