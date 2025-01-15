use alloc::{rc::Rc, vec::Vec};
use embassy_sync::mutex::Mutex;
use embassy_time::{with_timeout, Duration};

use super::STACK_SOCKET_COUNT;
use crate::{
    board::wifi::{
        ap::{Ap, ApConnectionState, ApController},
        ap_net_task,
        sta::{CommandQueue, InitialStaControllerState, Sta, StaConnectionState, StaController},
        sta_net_task,
    },
    task_control::{TaskControlToken, TaskController},
};
use embassy_executor::Spawner;
use embassy_futures::{
    join::join3,
    select::{select, Either},
};
use embassy_net::{Config, StackResources};
use esp_hal::{peripherals::WIFI, rng::Rng};
use esp_wifi::{wifi::WifiController, EspWifiController};
use macros as cardio;

pub(super) struct ApStaState {
    init: EspWifiController<'static>,
    connection_task_control: TaskController<(), ApStaTaskResources>,
    ap_net_task_control: TaskController<!>,
    sta_net_task_control: TaskController<!>,
    ap_handle: Ap,
    sta_handle: Sta,
}

impl ApStaState {
    pub(super) fn init(
        init: EspWifiController<'static>,
        ap_config: Config,
        sta_config: Config,
        wifi: &'static mut WIFI,
        mut rng: Rng,
        sta_resources: &'static mut StackResources<STACK_SOCKET_COUNT>,
        ap_resources: &'static mut StackResources<STACK_SOCKET_COUNT>,
        spawner: Spawner,
    ) -> Self {
        info!("Configuring AP-STA");

        let (ap_device, sta_device, controller) = unwrap!(esp_wifi::wifi::new_ap_sta(
            unsafe { core::mem::transmute(&init) },
            wifi
        ));

        info!("Starting AP-STA");

        let lower = rng.random() as u64;
        let upper = rng.random() as u64;

        let random_seed = upper << 32 | lower;

        let ap_ptr = ap_resources as *mut _;
        let sta_ptr = sta_resources as *mut _;

        let (ap_stack, ap_runner) =
            embassy_net::new(ap_device, ap_config, ap_resources, random_seed);
        let (sta_stack, sta_runner) =
            embassy_net::new(sta_device, sta_config, sta_resources, random_seed);
        let ap_net_task_control = TaskController::new();
        let sta_net_task_control = TaskController::new();
        let ap_state = Rc::new(ApConnectionState::new());
        let sta_state = Rc::new(StaConnectionState::new());
        let networks = Rc::new(Mutex::new(heapless::Vec::new()));
        let known_networks = Rc::new(Mutex::new(Vec::new()));
        let command_queue = Rc::new(CommandQueue::new());

        let connection_task_control = TaskController::from_resources(ApStaTaskResources {
            controller,
            ap_resources: ap_ptr,
            sta_resources: sta_ptr,
        });

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
        spawner.must_spawn(ap_net_task(ap_runner, ap_net_task_control.token()));
        spawner.must_spawn(sta_net_task(sta_runner, sta_net_task_control.token()));

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

    pub(super) async fn stop(
        mut self,
    ) -> (
        EspWifiController<'static>,
        &'static mut StackResources<STACK_SOCKET_COUNT>,
        &'static mut StackResources<STACK_SOCKET_COUNT>,
    ) {
        info!("Stopping AP-STA");
        let _ = join3(
            self.connection_task_control.stop(),
            self.ap_net_task_control.stop(),
            self.sta_net_task_control.stop(),
        )
        .await;

        let ap_resources = self.connection_task_control.resources_mut().ap_resources;
        let sta_resources = self.connection_task_control.resources_mut().sta_resources;
        let controller = &mut self.connection_task_control.resources_mut().controller;
        if matches!(controller.is_started(), Ok(true)) {
            unwrap!(controller.stop_async().await);
        }

        info!("Stopped AP-STA");

        (
            self.init,
            unsafe { unwrap!(ap_resources.as_mut()) },
            unsafe { unwrap!(sta_resources.as_mut()) },
        )
    }

    pub(crate) fn handles(&self) -> (&Ap, &Sta) {
        (&self.ap_handle, &self.sta_handle)
    }
}

struct ApStaTaskResources {
    controller: WifiController<'static>,
    ap_resources: *mut StackResources<STACK_SOCKET_COUNT>,
    sta_resources: *mut StackResources<STACK_SOCKET_COUNT>,
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
            ap_controller.setup(&mut resources.controller).await;

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
