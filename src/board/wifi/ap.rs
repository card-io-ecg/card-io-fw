use alloc::rc::Rc;
use core::sync::atomic::{AtomicU32, Ordering};
use embassy_futures::join::join;
use gui::widgets::wifi_access_point::WifiAccessPointState;

use crate::{
    board::wifi::net_task,
    task_control::{TaskControlToken, TaskController},
};
use embassy_executor::Spawner;
use embassy_net::{Runner, Stack};
use esp_radio::wifi::{
    ap::AccessPointConfig, AccessPointStationEventInfo, Config, Interface, WifiController,
};
use macros as cardio;

pub(super) struct ApConnectionState {
    client_count: AtomicU32,
}

impl ApConnectionState {
    pub(super) fn new() -> Self {
        Self {
            client_count: AtomicU32::new(0),
        }
    }
}

#[derive(Clone)]
pub struct Ap {
    pub(super) ap_stack: Stack<'static>,
    pub(super) state: Rc<ApConnectionState>,
}

impl Ap {
    pub fn is_active(&self) -> bool {
        self.ap_stack.is_link_up()
    }

    pub fn stack(&self) -> Stack<'static> {
        self.ap_stack.clone()
    }

    pub fn client_count(&self) -> u32 {
        self.state.client_count.load(Ordering::Acquire)
    }

    pub fn connection_state(&self) -> WifiAccessPointState {
        if self.client_count() > 0 {
            WifiAccessPointState::Connected
        } else {
            WifiAccessPointState::NotConnected
        }
    }
}

pub(super) struct ApState {
    connection_task_control: TaskController<(), ApTaskResources>,
    net_task_control: TaskController<()>,
    handle: Ap,
}

impl ApState {
    pub(super) fn init(
        controller: WifiController<'static>,
        ap_stack: Stack<'static>,
        ap_runner: Runner<'static, Interface<'static>>,
        spawner: Spawner,
    ) -> Self {
        info!("Starting AP");

        let state = Rc::new(ApConnectionState::new());

        let connection_task_control =
            TaskController::from_resources(ApTaskResources { controller });
        let net_task_control = TaskController::new();

        info!("Starting AP tasks");
        spawner.spawn(unwrap!(ap_task(
            ApController::new(state.clone()),
            connection_task_control.token(),
        )));
        spawner.spawn(unwrap!(net_task(ap_runner, net_task_control.token())));

        Self {
            connection_task_control,
            net_task_control,
            handle: Ap { ap_stack, state },
        }
    }

    pub(super) async fn stop(self) {
        info!("Stopping AP");
        let _ = join(
            self.connection_task_control.stop(),
            self.net_task_control.stop(),
        )
        .await;

        info!("Stopped AP");
    }

    pub(crate) fn handle(&self) -> &Ap {
        &self.handle
    }
}

struct ApTaskResources {
    controller: WifiController<'static>,
}
unsafe impl Send for ApTaskResources {}

pub(super) struct ApController {
    state: Rc<ApConnectionState>,
}

impl ApController {
    pub fn new(state: Rc<ApConnectionState>) -> Self {
        Self { state }
    }

    pub async fn setup(&mut self, controller: &mut WifiController<'static>) {
        info!("Configuring AP");

        let ap_config = Config::AccessPoint(
            AccessPointConfig::default()
                .with_ssid(alloc::string::String::from("Card/IO"))
                .with_max_connections(1),
        );
        unwrap!(controller.set_config(&ap_config));
    }

    pub fn handle_event(&mut self, event: AccessPointStationEventInfo) {
        match event {
            AccessPointStationEventInfo::Connected(_) => {
                let old_count = self.state.client_count.load(Ordering::Acquire);
                let new_count = old_count.saturating_add(1);
                self.state.client_count.store(new_count, Ordering::Relaxed);
                info!("Client connected, {} total", new_count);
            }
            AccessPointStationEventInfo::Disconnected(_) => {
                let old_count = self.state.client_count.load(Ordering::Acquire);
                let new_count = old_count.saturating_sub(1);
                self.state.client_count.store(new_count, Ordering::Relaxed);
                info!("Client disconnected, {} left", new_count);
            }
        }
    }
}

#[cardio::task]
async fn ap_task(
    mut ap_controller: ApController,
    mut task_control: TaskControlToken<(), ApTaskResources>,
) {
    task_control
        .run_cancellable(|resources| async {
            ap_controller.setup(&mut resources.controller).await;

            loop {
                let event = resources
                    .controller
                    .wait_for_access_point_connected_event_async()
                    .await
                    .unwrap();

                ap_controller.handle_event(event);
            }
        })
        .await;
}
