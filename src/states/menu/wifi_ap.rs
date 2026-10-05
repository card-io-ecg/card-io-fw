use core::{
    fmt::{Debug, Display},
    net::{IpAddr, Ipv4Addr, SocketAddr},
};

use alloc::{boxed::Box, rc::Rc};
use config_site::{
    data::{SharedWebContext, WebContext},
    ConfigSite,
};
use edge_http::{
    io::{
        server::{Connection, Handler, Server},
        Error,
    },
    Method,
};
use edge_nal::{TcpBind, TcpSplit, WithTimeout};
use edge_nal_embassy::{Tcp, TcpBuffers};
use embassy_executor::Spawner;
use embassy_time::{Duration, Ticker, Timer};
use embedded_graphics::Drawable;
use embedded_io_async::{Read, Write};
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

    let webserver_task_control = TaskController::new();
    spawner.spawn(unwrap!(webserver_task(
        ap.clone(),
        sta.clone(),
        web_context.clone(),
        webserver_task_control.token(),
    )));

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

    let _ = webserver_task_control.stop().await;

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
const SOCKET_TIMEOUT_MS: u32 = 10_000;
const KEEPALIVE_TIMEOUT_MS: u32 = 5_000;

struct WebserverResources {
    buffers: TcpBuffers<WEBSERVER_TASKS, 4096, 4096>,
    server: Server<WEBSERVER_TASKS, 2048, 24>,
}

#[cardio::task]
async fn webserver_task(
    ap: Ap,
    sta: Sta,
    context: Rc<SharedWebContext>,
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

            let handler = WebHandler {
                site: ConfigSite::new(&context, env!("FW_VERSION")),
                sta,
            };

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

/// Serves the config site, and the list of visible networks which needs the station interface.
struct WebHandler<'a> {
    site: ConfigSite<'a>,
    sta: Sta,
}

impl Handler for WebHandler<'_> {
    type Error<E>
        = Error<E>
    where
        E: Debug;

    async fn handle<T, const N: usize>(
        &self,
        task_id: impl Display + Copy,
        conn: &mut Connection<'_, T, N>,
    ) -> Result<(), Self::Error<T::Error>>
    where
        T: Read + Write + TcpSplit,
    {
        let headers = conn.headers()?;
        if !(matches!(headers.method, Method::Get) && headers.path == "/vn") {
            return self.site.handle(task_id, conn).await;
        }

        self.sta.scan().await;

        conn.initiate_response(200, None, &[("Content-Type", "text/plain; charset=utf-8")])
            .await?;

        let networks = self.sta.visible_networks().await;
        for network in networks.iter() {
            conn.write_all(network.ssid.as_str().as_bytes()).await?;
            conn.write_all(b"\n").await?;
        }

        Ok(())
    }
}
