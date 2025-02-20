use alloc::{rc::Rc, vec::Vec};
use embassy_sync::mutex::Mutex;
use embassy_time::{with_timeout, Duration};

use crate::{
    board::wifi::{
        ap::{Ap, ApConnectionState, ApController},
        sta::{CommandQueue, InitialStaControllerState, Sta, StaConnectionState, StaController},
    },
    task_control::{TaskControlToken, TaskController},
};
use embassy_executor::Spawner;
use embassy_futures::select::{select, Either};
use embassy_net::Stack;
use esp_hal::rng::Rng;
use esp_wifi::wifi::{
    AccessPointConfiguration, ClientConfiguration, Configuration, WifiController,
};
use macros as cardio;

pub(super) struct ApStaState {
    connection_task_control: TaskController<(), ApStaTaskResources>,
    ap_handle: Ap,
    sta_handle: Sta,
}

impl ApStaState {
    pub(super) fn init(
        controller: WifiController<'static>,
        ap_stack: Stack<'static>,
        sta_stack: Stack<'static>,
        rng: Rng,
        spawner: Spawner,
    ) -> Self {
        info!("Configuring AP-STA");

        let ap_state = Rc::new(ApConnectionState::new());
        let sta_state = Rc::new(StaConnectionState::new());
        let networks = Rc::new(Mutex::new(heapless::Vec::new()));
        let known_networks = Rc::new(Mutex::new(Vec::new()));
        let command_queue = Rc::new(CommandQueue::new());

        let connection_task_control =
            TaskController::from_resources(ApStaTaskResources { controller });

        info!("Starting AP-STA task");
        spawner.must_spawn(ap_sta_task(
            StaController::new(
                sta_state.clone(),
                networks.clone(),
                known_networks.clone(),
                sta_stack.clone(),
                command_queue.clone(),
                InitialStaControllerState::Idle,
            ),
            ApController::new(ap_state.clone()),
            connection_task_control.token(),
        ));

        Self {
            connection_task_control,

            ap_handle: Ap {
                ap_stack,
                state: ap_state,
            },
            sta_handle: Sta {
                sta_stack,
                networks,
                known_networks,
                state: sta_state,
                command_queue,
                rng,
            },
        }
    }

    pub(super) async fn stop(self) -> (WifiController<'static>, Stack<'static>, Stack<'static>) {
        info!("Stopping AP-STA");
        let _ = self.connection_task_control.stop().await;

        let mut controller = self.connection_task_control.unwrap().controller;
        if matches!(controller.is_started(), Ok(true)) {
            unwrap!(controller.stop_async().await);
        }

        info!("Stopped AP-STA");

        (
            controller,
            self.ap_handle.ap_stack,
            self.sta_handle.sta_stack,
        )
    }

    pub(crate) fn handles(&self) -> (&Ap, &Sta) {
        (&self.ap_handle, &self.sta_handle)
    }
}

struct ApStaTaskResources {
    controller: WifiController<'static>,
}
unsafe impl Send for ApStaTaskResources {}

const NO_TIMEOUT: Duration = Duration::MAX;

#[cardio::task]
async fn ap_sta_task(
    mut sta_controller: StaController,
    mut ap_controller: ApController,
    mut task_control: TaskControlToken<(), ApStaTaskResources>,
) {
    task_control
        .run_cancellable(|resources| async {
            let ap_config = AccessPointConfiguration {
                ssid: "Card/IO".try_into().unwrap(),
                max_connections: 1,
                ..Default::default()
            };
            let client_config = ClientConfiguration {
                ..Default::default()
            };
            unwrap!(resources
                .controller
                .set_configuration(&Configuration::Mixed(client_config, ap_config)));

            info!("Starting wifi");
            unwrap!(resources.controller.start_async().await);
            info!("Wifi started!");

            loop {
                let events = sta_controller.events() | ap_controller.events();

                let timeout = sta_controller.update(&mut resources.controller).await;

                let event_or_command = select(
                    async {
                        if timeout == NO_TIMEOUT {
                            Some(resources.controller.wait_for_events(events, false).await)
                        } else {
                            with_timeout(
                                timeout,
                                resources.controller.wait_for_events(events, false),
                            )
                            .await
                            .ok()
                        }
                    },
                    sta_controller.wait_for_command(),
                )
                .await;

                match event_or_command {
                    Either::First(Some(events)) => {
                        let mut resume = true;

                        resume &= sta_controller.handle_events(events);
                        resume &= ap_controller.handle_events(events);

                        if !resume {
                            return;
                        }
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
