use alloc::rc::Rc;
use core::sync::atomic::{AtomicU32, Ordering};
use enumset::EnumSet;
use gui::widgets::wifi_access_point::WifiAccessPointState;

use crate::{
    board::{
        hal::{peripherals::WIFI, Rng},
        wifi::{ap_net_task, StackWrapper},
    },
    task_control::{TaskControlToken, TaskController},
};
use embassy_executor::Spawner;
use embassy_futures::join::join;
use embassy_net::{Config, Stack};
use esp_wifi::{
    wifi::{
        AccessPointConfiguration, Configuration, WifiApDevice, WifiController, WifiDevice,
        WifiEvent,
    },
    EspWifiInitialization,
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
    pub(super) ap_stack: Rc<StackWrapper<WifiApDevice>>,
    pub(super) state: Rc<ApConnectionState>,
}

impl Ap {
    pub fn is_active(&self) -> bool {
        self.ap_stack.is_link_up()
    }

    pub fn stack(&self) -> &Stack<WifiDevice<'static, WifiApDevice>> {
        &self.ap_stack
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
    init: EspWifiInitialization,
    connection_task_control: TaskController<(), ApTaskResources>,
    net_task_control: TaskController<!>,
    handle: Ap,
}

impl ApState {
    pub(super) fn init(
        init: EspWifiInitialization,
        config: Config,
        wifi: &'static mut WIFI,
        rng: Rng,
        spawner: Spawner,
    ) -> Self {
        info!("Configuring AP");

        let (ap_device, controller) =
            unwrap!(esp_wifi::wifi::new_with_mode(&init, wifi, WifiApDevice));

        info!("Starting AP");

        let ap_stack = StackWrapper::new(ap_device, config, rng);
        let net_task_control = TaskController::new();
        let state = Rc::new(ApConnectionState::new());

        let connection_task_control =
            TaskController::from_resources(ApTaskResources { controller });

        info!("Starting AP task");
        spawner.must_spawn(ap_task(
            ApController::new(state.clone()),
            connection_task_control.token(),
        ));

        info!("Starting NET task");
        spawner.must_spawn(ap_net_task(ap_stack.clone(), net_task_control.token()));

        Self {
            init,
            net_task_control,
            connection_task_control,
            handle: Ap { ap_stack, state },
        }
    }

    pub(super) async fn stop(mut self) -> EspWifiInitialization {
        info!("Stopping AP");
        let _ = join(
            self.connection_task_control.stop(),
            self.net_task_control.stop(),
        )
        .await;

        let controller = &mut self.connection_task_control.resources_mut().controller;
        if matches!(controller.is_started(), Ok(true)) {
            unwrap!(controller.stop().await);
        }

        info!("Stopped AP");

        self.init
    }

    pub(crate) fn handle(&self) -> &Ap {
        &self.handle
    }
}

struct ApTaskResources {
    controller: WifiController<'static>,
}

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
            | WifiEvent::ApStaconnected
            | WifiEvent::ApStadisconnected
    }

    pub async fn setup(&mut self, controller: &mut WifiController<'static>) {
        info!("Configuring AP");

        let client_config = Configuration::AccessPoint(AccessPointConfiguration {
            ssid: "Card/IO".try_into().unwrap(),
            max_connections: 1,
            ..Default::default()
        });
        unwrap!(controller.set_configuration(&client_config));
    }

    pub fn handle_events(&mut self, events: EnumSet<WifiEvent>) -> bool {
        if events.contains(WifiEvent::ApStaconnected) {
            let old_count = self.state.client_count.load(Ordering::Acquire);
            let new_count = old_count.saturating_add(1);
            self.state.client_count.store(new_count, Ordering::Relaxed);
            info!("Client connected, {} total", new_count);
        }

        if events.contains(WifiEvent::ApStadisconnected) {
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
            unwrap!(resources.controller.start().await);
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
