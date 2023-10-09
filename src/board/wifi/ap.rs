use alloc::{boxed::Box, rc::Rc};
use core::sync::atomic::{AtomicU32, Ordering};
use gui::widgets::wifi::WifiState;

use crate::{
    board::{
        hal::{radio::Wifi, Rng},
        wifi::{net_task, StackWrapper},
    },
    task_control::{TaskControlToken, TaskController},
};
use embassy_executor::Spawner;
use embassy_futures::join::join;
use embassy_net::{Config, Stack};
use embedded_svc::wifi::{AccessPointConfiguration, Configuration, Wifi as _};
use esp_wifi::{
    wifi::{WifiController, WifiDevice, WifiEvent, WifiMode, WifiState as WifiStackState},
    EspWifiInitialization,
};
use macros as cardio;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ApConnectionState {
    NotConnected,
    Connected,
}

impl From<ApConnectionState> for WifiState {
    fn from(state: ApConnectionState) -> Self {
        match state {
            ApConnectionState::NotConnected => WifiState::NotConnected,
            ApConnectionState::Connected => WifiState::Connected,
        }
    }
}

#[derive(Clone)]
pub struct Ap {
    stack: Rc<StackWrapper>,
    client_count: Rc<AtomicU32>,
}

impl Ap {
    pub fn is_active(&self) -> bool {
        self.stack.is_link_up()
    }

    pub fn stack(&self) -> &Stack<WifiDevice<'static>> {
        &self.stack
    }

    pub fn client_count(&self) -> u32 {
        self.client_count.load(Ordering::Acquire)
    }

    pub fn connection_state(&self) -> ApConnectionState {
        if self.client_count() > 0 {
            ApConnectionState::Connected
        } else {
            ApConnectionState::NotConnected
        }
    }
}

pub(super) struct ApState {
    init: EspWifiInitialization,
    stack: Rc<StackWrapper>,
    connection_task_control: TaskController<(), ApTaskResources>,
    net_task_control: TaskController<!>,
    client_count: Rc<AtomicU32>,
}

impl ApState {
    pub(super) async fn init(
        init: EspWifiInitialization,
        config: Config,
        wifi: &'static mut Wifi,
        rng: Rng,
    ) -> Self {
        info!("Configuring AP");

        let (wifi_interface, controller) =
            unwrap!(esp_wifi::wifi::new_with_mode(&init, wifi, WifiMode::Ap));

        info!("Starting AP");
        let spawner = Spawner::for_current_executor().await;

        let stack = Rc::new(StackWrapper::new(wifi_interface, config, rng));
        let net_task_control = TaskController::new();
        let client_count = Rc::new(AtomicU32::new(0));

        let controller = Box::new(controller);

        let connection_task_control =
            TaskController::from_resources(ApTaskResources { controller });

        info!("Starting AP task");
        spawner.must_spawn(ap_task(
            connection_task_control.token(),
            client_count.clone(),
        ));

        info!("Starting NET task");
        spawner.must_spawn(net_task(stack.clone(), net_task_control.token()));

        Self {
            init,
            stack,
            net_task_control,
            client_count,
            connection_task_control,
        }
    }

    pub(super) async fn stop(self) -> EspWifiInitialization {
        info!("Stopping AP");
        let _ = join(
            self.connection_task_control.stop(),
            self.net_task_control.stop(),
        )
        .await;

        let mut controller = self.connection_task_control.unwrap().controller;
        if matches!(controller.is_started(), Ok(true)) {
            unwrap!(controller.stop().await);
        }

        info!("Stopped AP");

        self.init
    }

    pub(super) fn is_running(&self) -> bool {
        if self.net_task_control.has_exited() {
            return false;
        }

        if self.connection_task_control.has_exited() {
            return false;
        }

        true
    }

    pub(crate) fn handle(&self) -> Ap {
        Ap {
            stack: self.stack.clone(),
            client_count: self.client_count.clone(),
        }
    }
}

struct ApTaskResources {
    controller: Box<WifiController<'static>>,
}

#[cardio::task]
async fn ap_task(
    mut task_control: TaskControlToken<(), ApTaskResources>,
    client_count: Rc<AtomicU32>,
) {
    task_control
        .run_cancellable(|resources| async {
            let controller = &mut resources.controller;
            info!("Start connection task");

            let client_config = Configuration::AccessPoint(AccessPointConfiguration {
                ssid: "Card/IO".into(),
                max_connections: 1,
                ..Default::default()
            });
            unwrap!(controller.set_configuration(&client_config));
            info!("Starting wifi");

            unwrap!(controller.start().await);
            info!("Wifi started!");

            loop {
                if let WifiStackState::ApStart
                | WifiStackState::ApStaConnected
                | WifiStackState::ApStaDisconnected = esp_wifi::wifi::get_wifi_state()
                {
                    let events = controller
                        .wait_for_events(
                            WifiEvent::ApStop
                                | WifiEvent::ApStaconnected
                                | WifiEvent::ApStadisconnected,
                            false,
                        )
                        .await;

                    if events.contains(WifiEvent::ApStaconnected) {
                        let old_count = client_count.fetch_add(1, Ordering::Release);
                        info!("Client connected, {} total", old_count + 1);
                    }
                    if events.contains(WifiEvent::ApStadisconnected) {
                        let old_count = client_count.fetch_sub(1, Ordering::Release);
                        info!("Client disconnected, {} left", old_count - 1);
                    }
                    if events.contains(WifiEvent::ApStop) {
                        info!("AP stopped");
                        client_count.store(0, Ordering::Relaxed);
                        return;
                    }

                    info!("Event processing done");
                }
            }
        })
        .await;
}
