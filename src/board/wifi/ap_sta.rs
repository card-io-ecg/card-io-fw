use alloc::{rc::Rc, vec::Vec};
use embassy_sync::mutex::Mutex;
use embassy_time::{with_timeout, Duration};

use crate::{
    board::wifi::{
        ap::{Ap, ApConnectionState, ApController},
        ap_net_task,
        sta::{CommandQueue, InitialStaControllerState, Sta, StaConnectionState, StaController},
        sta_net_task, StackWrapper,
    },
    task_control::{TaskControlToken, TaskController},
};
use embassy_executor::Spawner;
use embassy_futures::{
    join::join3,
    select::{select, Either},
};
use embassy_net::Config;
use esp_hal::{peripherals::WIFI, Rng};
use esp_wifi::{wifi::WifiController, EspWifiInitialization};
use macros as cardio;

pub(super) struct ApStaState {
    init: EspWifiInitialization,
    connection_task_control: TaskController<(), ApStaTaskResources>,
    ap_net_task_control: TaskController<!>,
    sta_net_task_control: TaskController<!>,
    ap_handle: Ap,
    sta_handle: Sta,
}

impl ApStaState {
    pub(super) fn init(
        init: EspWifiInitialization,
        ap_config: Config,
        sta_config: Config,
        wifi: &'static mut WIFI,
        rng: Rng,
        spawner: Spawner,
    ) -> Self {
        info!("Configuring AP-STA");

        let (ap_device, sta_device, controller) = unwrap!(esp_wifi::wifi::new_ap_sta(&init, wifi));

        info!("Starting AP-STA");

        let ap_stack = StackWrapper::new(ap_device, ap_config, rng);
        let sta_stack = StackWrapper::new(sta_device, sta_config, rng);
        let ap_net_task_control = TaskController::new();
        let sta_net_task_control = TaskController::new();
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

        info!("Starting NET tasks");
        spawner.must_spawn(ap_net_task(ap_stack.clone(), ap_net_task_control.token()));
        spawner.must_spawn(sta_net_task(
            sta_stack.clone(),
            sta_net_task_control.token(),
        ));

        Self {
            init,
            connection_task_control,
            ap_net_task_control,
            sta_net_task_control,

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

    pub(super) async fn stop(mut self) -> EspWifiInitialization {
        info!("Stopping AP-STA");
        let _ = join3(
            self.connection_task_control.stop(),
            self.ap_net_task_control.stop(),
            self.sta_net_task_control.stop(),
        )
        .await;

        let controller = &mut self.connection_task_control.resources_mut().controller;
        if matches!(controller.is_started(), Ok(true)) {
            unwrap!(controller.stop().await);
        }

        info!("Stopped AP-STA");

        self.init
    }

    pub(crate) fn handles(&self) -> (&Ap, &Sta) {
        (&self.ap_handle, &self.sta_handle)
    }
}

struct ApStaTaskResources {
    controller: WifiController<'static>,
}

const NO_TIMEOUT: Duration = Duration::MAX;

#[cardio::task]
async fn ap_sta_task(
    mut sta_controller: StaController,
    mut ap_controller: ApController,
    mut task_control: TaskControlToken<(), ApStaTaskResources>,
) {
    task_control
        .run_cancellable(|resources| async {
            ap_controller.setup(&mut resources.controller).await;

            info!("Starting wifi");
            unwrap!(resources.controller.start().await);
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
