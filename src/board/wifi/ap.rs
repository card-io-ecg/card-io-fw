use alloc::rc::Rc;
use core::sync::atomic::{AtomicU32, Ordering};
use gui::widgets::wifi::WifiState;

use crate::{
    board::{
        hal::{radio::Wifi, Rng},
        wifi::net_task,
    },
    task_control::{TaskControlToken, TaskController},
};
use embassy_executor::Spawner;
use embassy_futures::join::join;
use embassy_net::{Config, Stack, StackResources};
use embassy_sync::{blocking_mutex::raw::NoopRawMutex, mutex::Mutex};
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
    stack: Rc<Stack<WifiDevice<'static>>>,
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
    controller: Rc<Mutex<NoopRawMutex, WifiController<'static>>>,
    stack: Rc<Stack<WifiDevice<'static>>>,
    connection_task_control: TaskController<()>,
    net_task_control: TaskController<!>,
    client_count: Rc<AtomicU32>,
    started: bool,
}

impl ApState {
    pub(super) fn init(
        init: EspWifiInitialization,
        config: Config,
        wifi: &'static mut Wifi,
        resources: &'static mut StackResources<3>,
        mut rng: Rng,
    ) -> Self {
        info!("Configuring AP");

        let (wifi_interface, controller) =
            unwrap!(esp_wifi::wifi::new_with_mode(&init, wifi, WifiMode::Ap));

        let lower = rng.random() as u64;
        let upper = rng.random() as u64;

        let random_seed = upper << 32 | lower;

        Self {
            init,
            controller: Rc::new(Mutex::new(controller)),
            stack: Rc::new(Stack::new(wifi_interface, config, resources, random_seed)),
            connection_task_control: TaskController::new(),
            net_task_control: TaskController::new(),
            client_count: Rc::new(AtomicU32::new(0)),
            started: false,
        }
    }

    pub(super) fn unwrap(self) -> EspWifiInitialization {
        self.init
    }

    pub(super) async fn start(&mut self) -> Ap {
        if !self.started {
            info!("Starting AP");
            let spawner = Spawner::for_current_executor().await;

            info!("Starting AP task");
            spawner.must_spawn(ap_task(
                self.controller.clone(),
                self.connection_task_control.token(),
                self.client_count.clone(),
            ));
            info!("Starting NET task");
            spawner.must_spawn(net_task(self.stack.clone(), self.net_task_control.token()));

            self.started = true;
        }

        Ap {
            stack: self.stack.clone(),
            client_count: self.client_count.clone(),
        }
    }

    pub(super) async fn stop(&mut self) {
        if self.started {
            info!("Stopping AP");
            let _ = join(
                self.connection_task_control.stop_from_outside(),
                self.net_task_control.stop_from_outside(),
            )
            .await;

            if matches!(self.controller.lock().await.is_started(), Ok(true)) {
                unwrap!(self.controller.lock().await.stop().await);
            }

            info!("Stopped AP");
            self.started = false;
        }
    }

    pub(super) fn is_running(&self) -> bool {
        !self.connection_task_control.has_exited() && !self.net_task_control.has_exited()
    }

    pub(crate) fn handle(&self) -> Option<Ap> {
        self.started.then_some(Ap {
            stack: self.stack.clone(),
            client_count: self.client_count.clone(),
        })
    }
}

#[cardio::task]
pub(super) async fn ap_task(
    controller: Rc<Mutex<NoopRawMutex, WifiController<'static>>>,
    mut task_control: TaskControlToken<()>,
    client_count: Rc<AtomicU32>,
) {
    task_control
        .run_cancellable(async {
            info!("Start connection task");

            let client_config = Configuration::AccessPoint(AccessPointConfiguration {
                ssid: "Card/IO".into(),
                max_connections: 1,
                ..Default::default()
            });
            unwrap!(controller.lock().await.set_configuration(&client_config));
            info!("Starting wifi");

            unwrap!(controller.lock().await.start().await);
            info!("Wifi started!");

            loop {
                if let WifiStackState::ApStart
                | WifiStackState::ApStaConnected
                | WifiStackState::ApStaDisconnected = esp_wifi::wifi::get_wifi_state()
                {
                    let events = controller
                        .lock()
                        .await
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
