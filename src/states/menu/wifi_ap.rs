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
    screens::{
        screen::Screen,
        wifi_ap::{ApMenuEvents, WifiApScreen},
    },
    widgets::wifi::WifiState,
};
use macros as cardio;

use crate::{
    board::{initialized::Board, wifi::ap::Ap},
    states::{
        menu::AppMenu, TouchInputShaper, MENU_IDLE_DURATION, MIN_FRAME_TIME, WEBSERVER_TASKS,
    },
    task_control::{TaskControlToken, TaskController},
    timeout::Timeout,
    AppState,
};

pub async fn wifi_ap(board: &mut Board) -> AppState {
    let Some(ap) = board.enable_wifi_ap().await else {
        // FIXME: Show error screen
        return AppState::Menu(AppMenu::Main);
    };

    let spawner = Spawner::for_current_executor().await;

    let context = Rc::new(SharedWebContext::new(WebContext {
        known_networks: board.config.known_networks.clone(),
        backend_url: board.config.backend_url.clone(),
    }));

    let webserver_task_control = [(); WEBSERVER_TASKS].map(|_| TaskController::new());
    for control in webserver_task_control.iter() {
        spawner.must_spawn(webserver_task(ap.clone(), context.clone(), control.token()));
    }

    let mut screen = Screen {
        content: WifiApScreen::new(),

        status_bar: board.status_bar(),
    };

    let mut ticker = Ticker::every(MIN_FRAME_TIME);
    let mut exit_timer = Timeout::new(MENU_IDLE_DURATION);
    let mut input = TouchInputShaper::new();

    while board.wifi.ap_running() {
        input.update(&mut board.frontend);
        let is_touched = input.is_touched();

        #[cfg(feature = "battery_max17055")]
        // We only enable this check for fuel gauges because enabling wifi modifies ADC readings
        // and the board would shut down immediately.
        if board.battery_monitor.is_low() {
            break;
        }

        screen.status_bar = board.status_bar();

        let connection_state: WifiState = ap.connection_state().into();
        if connection_state != WifiState::Connected {
            // We start counting when the last client disconnects, and we reset on interaction.
            if screen.content.state == WifiState::Connected || is_touched {
                exit_timer.reset();
            }

            if exit_timer.is_elapsed() {
                break;
            }
            screen.content.timeout = Some(exit_timer.remaining().as_secs() as u8);
        } else {
            screen.content.timeout = None;
        }

        screen.content.state = connection_state;

        #[allow(irrefutable_let_patterns)]
        if let Some(ApMenuEvents::Exit) = screen.content.menu.interact(is_touched) {
            break;
        }

        board.display.frame(|display| screen.draw(display)).await;

        ticker.next().await;
    }

    for control in webserver_task_control {
        let _ = control.stop().await;
    }

    board.disable_wifi().await;

    {
        let context = context.lock().await;
        board.update_config(|config| {
            if context.known_networks != config.known_networks {
                config.known_networks.clone_from(&context.known_networks);
            }
            if context.backend_url != config.backend_url {
                config.backend_url.clone_from(&context.backend_url);
            }
        });
    }

    board.save_config().await;

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
