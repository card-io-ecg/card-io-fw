use alloc::{boxed::Box, rc::Rc};
use bad_server::{
    connector::{
        embassy_net_compat::{listen, AcceptQueue, TcpConnection},
        Connection,
    },
    handler::RequestHandler,
    request::Request,
    response::ResponseStatus,
    HandleError,
};
use config_site::{
    self,
    data::{SharedWebContext, WebContext},
};
use embassy_executor::Spawner;
use embassy_time::{Duration, Ticker, Timer};
use embedded_graphics::Drawable;
use gui::{
    screens::wifi_ap::{ApMenuEvents, WifiApScreen},
    widgets::wifi_access_point::WifiAccessPointState,
};
use macros as cardio;

use crate::{
    board::{
        initialized::Context,
        wifi::{ap::Ap, sta::Sta},
    },
    states::{
        menu::AppMenu, TouchInputShaper, MENU_FRAME_TIME, MENU_IDLE_DURATION, WEBSERVER_TASKS,
    },
    task_control::{TaskControlToken, TaskController},
    timeout::Timeout,
    AppState,
};

pub async fn wifi_ap(context: &mut Context) -> AppState {
    let Some((ap, sta)) = context.enable_wifi_ap_sta().await else {
        // FIXME: Show error screen
        return AppState::Menu(AppMenu::Main);
    };

    let spawner = unsafe { Spawner::for_current_executor().await };

    let web_context = Rc::new(SharedWebContext::new(WebContext {
        known_networks: context.config.known_networks.clone(),
        backend_url: context.config.backend_url.clone(),
    }));

    // A port can only have a single listener, so one task accepts the connections and hands them
    // out to the webserver tasks.
    let connection_queue = Rc::new(ConnectionQueue::new());

    let listener_task_control = TaskController::new();
    spawner.spawn(unwrap!(listener_task(
        ap.clone(),
        connection_queue.clone(),
        listener_task_control.token(),
    )));

    let webserver_task_control = [(); WEBSERVER_TASKS].map(|_| TaskController::new());
    for control in webserver_task_control.iter() {
        spawner.spawn(unwrap!(webserver_task(
            ap.clone(),
            sta.clone(),
            web_context.clone(),
            connection_queue.clone(),
            control.token(),
        )));
    }

    let mut screen = WifiApScreen::new();

    let mut ticker = Ticker::every(MENU_FRAME_TIME);
    let mut exit_timer = Timeout::new(MENU_IDLE_DURATION);
    let mut input = TouchInputShaper::new();

    let mut prev_timeout = 0;

    loop {
        input.update(&mut context.frontend);
        let is_touched = input.is_touched();

        // We only enable this check for fuel gauges because enabling wifi modifies ADC readings
        // and the board would shut down immediately.
        if context.battery_monitor.is_low() {
            break;
        }

        let timeout = exit_timer.remaining().as_secs() as u8;
        let connection_state: WifiAccessPointState = ap.connection_state().into();
        if connection_state != WifiAccessPointState::Connected {
            // We start counting when the last client disconnects, and we reset on interaction.
            if screen.state == WifiAccessPointState::Connected || is_touched {
                exit_timer.reset();
            }

            if exit_timer.is_elapsed() {
                break;
            }
            screen.timeout = Some(timeout);
        } else {
            screen.timeout = None;
        }

        let changed = connection_state != screen.state || prev_timeout != timeout;

        prev_timeout = timeout;
        screen.state = connection_state;

        #[allow(irrefutable_let_patterns)]
        if let Some(ApMenuEvents::Exit) = screen.menu.interact(is_touched) {
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

        ticker.next().await;
    }

    let _ = listener_task_control.stop().await;
    for control in webserver_task_control {
        let _ = control.stop().await;
    }

    context.disable_wifi().await;

    {
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

    context.save_config().await;

    AppState::Menu(AppMenu::Main)
}

const WEBSERVER_PORT: u16 = 8080;

/// Hands accepted connections from the listener task to the webserver tasks.
type ConnectionQueue = AcceptQueue<1>;

#[cardio::task]
async fn listener_task(ap: Ap, queue: Rc<ConnectionQueue>, mut task_control: TaskControlToken<()>) {
    info!("Started listener task");
    task_control
        .run_cancellable(|_| async {
            while !ap.is_active() {
                Timer::after(Duration::from_millis(500)).await;
            }

            if let Err(e) = listen(ap.stack(), WEBSERVER_PORT, &queue).await {
                warn!("Listener error: {:?}", e);
            }
        })
        .await;
    info!("Stopped listener task");
}

#[derive(Clone, Copy)]
struct WebserverResources {
    tx_buffer: [u8; 4096],
    rx_buffer: [u8; 4096],
    request_buffer: [u8; 2048],
}

#[cardio::task(pool_size = WEBSERVER_TASKS)]
async fn webserver_task(
    ap: Ap,
    sta: Sta,
    context: Rc<SharedWebContext>,
    queue: Rc<ConnectionQueue>,
    mut task_control: TaskControlToken<()>,
) {
    info!("Started webserver task");
    task_control
        .run_cancellable(|_| async {
            let mut resources = Box::new(WebserverResources {
                tx_buffer: [0; 4096],
                rx_buffer: [0; 4096],
                request_buffer: [0; 2048],
            });

            let mut socket = unwrap!(TcpConnection::new(
                ap.stack(),
                &mut resources.rx_buffer,
                &mut resources.tx_buffer,
                queue.dyn_receiver(),
            ));
            socket.set_timeout(Some(Duration::from_secs(10)));

            config_site::create(&context, env!("FW_VERSION"))
                .with_handler(RequestHandler::get("/vn", VisibleNetworks { sta }))
                .with_request_buffer(&mut resources.request_buffer[..])
                .with_header_count::<24>()
                .serve(&mut socket)
                .await;
        })
        .await;
    info!("Stopped webserver task");
}

struct VisibleNetworks {
    sta: Sta,
}

impl<C: Connection> RequestHandler<C> for VisibleNetworks {
    async fn handle(&self, request: Request<'_, '_, C>) -> Result<(), HandleError<C>> {
        self.sta.scan().await;

        let response = request.start_response(ResponseStatus::Ok).await?;
        let mut response = response.start_chunked_body().await?;

        let networks = self.sta.visible_networks().await;
        for network in networks.iter() {
            response.write(network.ssid.as_str()).await?;
            response.write("\n").await?;
        }

        response.end_chunked_response().await
    }
}
