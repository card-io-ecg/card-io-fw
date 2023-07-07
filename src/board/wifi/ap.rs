use alloc::rc::Rc;
use core::{
    mem::MaybeUninit,
    ptr::{self, addr_of_mut},
    sync::atomic::{AtomicU32, Ordering},
};

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
use embedded_hal_old::prelude::_embedded_hal_blocking_rng_Read;
use embedded_svc::wifi::{AccessPointConfiguration, Configuration, Wifi as _};
use esp_wifi::{
    wifi::{WifiController, WifiDevice, WifiEvent, WifiMode, WifiState},
    EspWifiInitialization,
};

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
        this: &mut MaybeUninit<Self>,
        init: EspWifiInitialization,
        config: Config,
        wifi: &'static mut Wifi,
        resources: &'static mut StackResources<3>,
        mut rng: Rng,
    ) {
        log::info!("Configuring AP");

        let this = this.as_mut_ptr();

        let (wifi_interface, controller) =
            esp_wifi::wifi::new_with_mode(&init, wifi, WifiMode::Ap).unwrap();

        let mut seed = [0; 8];
        rng.read(&mut seed).unwrap();

        unsafe {
            (*this).init = init;
            ptr::write(
                addr_of_mut!((*this).controller),
                Rc::new(Mutex::new(controller)),
            );
            ptr::write(
                addr_of_mut!((*this).stack),
                Rc::new(Stack::new(
                    wifi_interface,
                    config,
                    resources,
                    u64::from_le_bytes(seed),
                )),
            );
            ptr::write(
                addr_of_mut!((*this).connection_task_control),
                TaskController::new(),
            );
            ptr::write(
                addr_of_mut!((*this).net_task_control),
                TaskController::new(),
            );
            ptr::write(
                addr_of_mut!((*this).client_count),
                Rc::new(AtomicU32::new(0)),
            );
            (*this).started = false;
        }
    }

    pub(super) fn unwrap(self) -> EspWifiInitialization {
        self.init
    }

    pub(super) async fn start(&mut self) -> Ap {
        if !self.started {
            log::info!("Starting AP");
            let spawner = Spawner::for_current_executor().await;

            log::info!("Starting AP task");
            spawner.must_spawn(ap_task(
                self.controller.clone(),
                self.connection_task_control.token(),
                self.client_count.clone(),
            ));
            log::info!("Starting NET task");
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
            log::info!("Stopping AP");
            let _ = join(
                self.connection_task_control.stop_from_outside(),
                self.net_task_control.stop_from_outside(),
            )
            .await;

            if matches!(self.controller.lock().await.is_started(), Ok(true)) {
                self.controller.lock().await.stop().await.unwrap();
            }

            log::info!("Stopped AP");
            self.started = false;
        }
    }

    pub(super) fn is_running(&self) -> bool {
        !self.connection_task_control.has_exited() && !self.net_task_control.has_exited()
    }
}

#[embassy_executor::task]
pub(super) async fn ap_task(
    controller: Rc<Mutex<NoopRawMutex, WifiController<'static>>>,
    mut task_control: TaskControlToken<()>,
    client_count: Rc<AtomicU32>,
) {
    task_control
        .run_cancellable(async {
            log::info!("Start connection task");
            log::debug!(
                "Device capabilities: {:?}",
                controller.lock().await.get_capabilities()
            );

            loop {
                if !matches!(controller.lock().await.is_started(), Ok(true)) {
                    let client_config = Configuration::AccessPoint(AccessPointConfiguration {
                        ssid: "Card/IO".into(),
                        max_connections: 1,
                        ..Default::default()
                    });
                    controller
                        .lock()
                        .await
                        .set_configuration(&client_config)
                        .unwrap();
                    log::info!("Starting wifi");

                    controller.lock().await.start().await.unwrap();
                    log::info!("Wifi started!");
                }

                if let WifiState::ApStart
                | WifiState::ApStaConnected
                | WifiState::ApStaDisconnected = esp_wifi::wifi::get_wifi_state()
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
                        log::info!("Client connected, {} total", old_count + 1);
                    }
                    if events.contains(WifiEvent::ApStadisconnected) {
                        let old_count = client_count.fetch_sub(1, Ordering::Release);
                        log::info!("Client disconnected, {} left", old_count - 1);
                    }
                    if events.contains(WifiEvent::ApStop) {
                        log::info!("AP stopped");
                        return;
                    }

                    log::info!("Event processing done");
                }
            }
        })
        .await;
}
