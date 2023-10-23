use alloc::{boxed::Box, rc::Rc};
use config_site::{
    self,
    data::{SharedWebContext, WebContext},
};
use embassy_executor::Spawner;
use embassy_net::tcp::TcpSocket;
use embassy_time::{Duration, Ticker, Timer};
use embedded_graphics::Drawable;
use gui::{
    screens::wifi_ap::{ApMenuEvents, WifiApScreen},
    widgets::wifi_access_point::WifiAccessPointState,
};
use macros as cardio;

use crate::{
    board::{initialized::Context, wifi::ap::Ap},
    states::{
        menu::AppMenu, TouchInputShaper, MENU_IDLE_DURATION, MIN_FRAME_TIME, WEBSERVER_TASKS,
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

    let spawner = Spawner::for_current_executor().await;

    let web_context = Rc::new(SharedWebContext::new(WebContext {
        known_networks: context.config.known_networks.clone(),
        backend_url: context.config.backend_url.clone(),
    }));

    let webserver_task_control = [(); WEBSERVER_TASKS].map(|_| TaskController::new());
    for control in webserver_task_control.iter() {
        spawner.must_spawn(webserver_task(
            ap.clone(),
            web_context.clone(),
            control.token(),
        ));
    }

    let mut screen = WifiApScreen::new();

    let mut ticker = Ticker::every(MIN_FRAME_TIME);
    let mut exit_timer = Timeout::new(MENU_IDLE_DURATION);
    let mut input = TouchInputShaper::new();

    loop {
        input.update(&mut context.frontend);
        let is_touched = input.is_touched();

        #[cfg(feature = "battery_max17055")]
        // We only enable this check for fuel gauges because enabling wifi modifies ADC readings
        // and the board would shut down immediately.
        if context.battery_monitor.is_low() {
            break;
        }

        let connection_state: WifiAccessPointState = ap.connection_state().into();
        if connection_state != WifiAccessPointState::Connected {
            // We start counting when the last client disconnects, and we reset on interaction.
            if screen.state == WifiAccessPointState::Connected || is_touched {
                exit_timer.reset();
            }

            if exit_timer.is_elapsed() {
                break;
            }
            screen.timeout = Some(exit_timer.remaining().as_secs() as u8);
        } else {
            screen.timeout = None;
        }

        screen.state = connection_state;

        #[allow(irrefutable_let_patterns)]
        if let Some(ApMenuEvents::Exit) = screen.menu.interact(is_touched) {
            break;
        }

        context
            .with_status_bar(|display| screen.draw(display))
            .await;

        ticker.next().await;
    }

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

#[derive(Clone, Copy)]
struct WebserverResources {
    tx_buffer: [u8; 4096],
    rx_buffer: [u8; 4096],
    request_buffer: [u8; 2048],
}

#[cardio::task(pool_size = WEBSERVER_TASKS)]
async fn webserver_task(
    ap: Ap,
    context: Rc<SharedWebContext>,
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

            while !ap.is_active() {
                Timer::after(Duration::from_millis(500)).await;
            }

            let mut socket = TcpSocket::new(
                ap.stack(),
                &mut resources.rx_buffer,
                &mut resources.tx_buffer,
            );
            socket.set_timeout(Some(Duration::from_secs(10)));

            config_site::create(&context, env!("FW_VERSION"))
                .with_request_buffer(&mut resources.request_buffer[..])
                .with_header_count::<24>()
                .listen(&mut socket, 8080)
                .await;
        })
        .await;
    info!("Stopped webserver task");
}
