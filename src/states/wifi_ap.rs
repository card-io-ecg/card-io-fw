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
use embassy_net::{tcp::TcpSocket, Config, Ipv4Address, Ipv4Cidr, Stack, StaticConfigV4};
use embassy_time::{Duration, Instant, Ticker, Timer};
use embedded_graphics::Drawable;
use esp_wifi::wifi::WifiDevice;
use gui::screens::wifi_ap::{ApMenuEvents, WifiApScreen, WifiApScreenState};

use crate::{
    board::{
        initialized::Board,
        wifi::{as_static_mut, as_static_ref},
    },
    states::{WebserverResources, BIG_OBJECTS, MIN_FRAME_TIME, WEBSERVER_TASKS},
    task_control::TaskController,
    AppState,
};

pub async fn wifi_ap(board: &mut Board) -> AppState {
    board.wifi.initialize(&board.clocks);

    let wifi_resources = unsafe { BIG_OBJECTS.as_wifi_ap_resources() };
    let stack = board
        .wifi
        .configure_ap(
            Config::ipv4_static(StaticConfigV4 {
                address: Ipv4Cidr::new(Ipv4Address::new(192, 168, 2, 1), 24),
                gateway: Some(Ipv4Address::from_bytes(&[192, 168, 2, 1])),
                dns_servers: Default::default(),
            }),
            &mut wifi_resources.stack_resources,
        )
        .await;

    let spawner = Spawner::for_current_executor().await;

    let webserver_task_control = [TaskController::DEFAULT; WEBSERVER_TASKS];

    let context = SharedWebContext::new(WebContext {
        known_networks: board.config.known_networks.clone(),
    });

    unsafe {
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
    let mut last_interaction = Instant::now();
    while board.wifi.ap_running() {
        let battery_data = board.battery_monitor.battery_data().await;

        #[cfg(feature = "battery_max17055")]
        // We only enable this check for fuel gauges because enabling wifi modifies ADC readings
        // and the board would shut down immediately.
        if let Some(battery) = battery_data {
            if battery.is_low {
                break;
            }
        }

        screen.battery_data = battery_data;

        screen.state = if board.wifi.ap_client_count().await > 0 {
            WifiApScreenState::Connected
        } else {
            if screen.state == WifiApScreenState::Connected || board.frontend.is_touched() {
                last_interaction = Instant::now();
            }

            if last_interaction.elapsed() > Duration::from_secs(30) {
                break;
            }
            WifiApScreenState::Idle
        };

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

    // NB: we must not consume the array here, as we have static references to it.
    for control in webserver_task_control.iter() {
        let _ = control.stop_from_outside().await;
    }

    board.wifi.stop_if().await;

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
            socket.set_timeout(Some(Duration::from_secs(10)));

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
