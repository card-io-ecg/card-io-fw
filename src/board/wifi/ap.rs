use alloc::rc::Rc;
use core::sync::atomic::{AtomicU32, Ordering};
use enumset::EnumSet;
use gui::widgets::wifi_access_point::WifiAccessPointState;

use crate::task_control::{TaskControlToken, TaskController};
use embassy_executor::Spawner;
use embassy_net::Stack;
use esp_radio::wifi::{AccessPointConfig, ModeConfig, WifiController, WifiEvent};
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
    handle: Ap,
}

impl ApState {
    pub(super) fn init(
        controller: WifiController<'static>,
        ap_stack: Stack<'static>,
        spawner: Spawner,
    ) -> Self {
        info!("Starting AP");

        let state = Rc::new(ApConnectionState::new());

        let connection_task_control =
            TaskController::from_resources(ApTaskResources { controller });

        info!("Starting AP task");
        spawner.must_spawn(ap_task(
            ApController::new(state.clone()),
            connection_task_control.token(),
        ));

        Self {
            connection_task_control,
            handle: Ap { ap_stack, state },
        }
    }

    pub(super) async fn stop(self) -> (WifiController<'static>, Stack<'static>) {
        info!("Stopping AP");
        let _ = self.connection_task_control.stop().await;

        let mut controller = self.connection_task_control.unwrap().controller;
        if matches!(controller.is_started(), Ok(true)) {
            unwrap!(controller.stop_async().await);
        }

        info!("Stopped AP");

        (controller, self.handle.ap_stack)
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

    pub fn events(&self) -> EnumSet<WifiEvent> {
        WifiEvent::ApStart
            | WifiEvent::ApStop
            | WifiEvent::ApStaConnected
            | WifiEvent::ApStaDisconnected
    }

    pub async fn setup(&mut self, controller: &mut WifiController<'static>) {
        info!("Configuring AP");

        let ap_config = ModeConfig::AccessPoint(
            AccessPointConfig::default()
                .with_ssid(alloc::string::String::from("Card/IO"))
                .with_max_connections(1),
        );
        unwrap!(controller.set_config(&ap_config));
    }

    pub fn handle_events(&mut self, events: EnumSet<WifiEvent>) -> bool {
        if events.contains(WifiEvent::ApStaConnected) {
            let old_count = self.state.client_count.load(Ordering::Acquire);
            let new_count = old_count.saturating_add(1);
            self.state.client_count.store(new_count, Ordering::Relaxed);
            info!("Client connected, {} total", new_count);
        }

        if events.contains(WifiEvent::ApStaDisconnected) {
            let old_count = self.state.client_count.load(Ordering::Acquire);
            let new_count = old_count.saturating_sub(1);
            self.state.client_count.store(new_count, Ordering::Relaxed);
            info!("Client disconnected, {} left", new_count);
        }

        if events.contains(WifiEvent::ApStop) {
            info!("AP stopped");
            self.state.client_count.store(0, Ordering::Relaxed);
            return false;
        }

        true
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

            info!("Starting wifi");
            unwrap!(resources.controller.start_async().await);
            info!("Wifi started!");

            loop {
                let events = ap_controller.events();

                let events = resources.controller.wait_for_events(events, false).await;

                if !ap_controller.handle_events(events) {
                    return;
                }
            }
        })
        .await;
}
