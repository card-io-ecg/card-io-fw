use core::{
    mem::MaybeUninit,
    ptr::{self, addr_of_mut},
};

use crate::{
    board::{
        hal::radio::Wifi,
        wifi::{as_static_mut, as_static_ref, net_task},
    },
    task_control::TaskController,
};
use embassy_executor::Spawner;
use embassy_futures::join::join;
use embassy_net::{Config, Stack, StackResources};
use embassy_sync::{blocking_mutex::raw::NoopRawMutex, mutex::Mutex};
use embedded_svc::wifi::{AccessPointConfiguration, Configuration, Wifi as _};
use esp_wifi::{
    wifi::{WifiController, WifiDevice, WifiEvent, WifiMode, WifiState},
    EspWifiInitialization,
};

pub(super) struct ApState {
    init: EspWifiInitialization,
    controller: WifiController<'static>,
    stack: Stack<WifiDevice<'static>>,
    connection_task_control: TaskController<()>,
    net_task_control: TaskController<!>,
    client_count: Mutex<NoopRawMutex, u32>,
    started: bool,
}

impl ApState {
    pub(super) fn init(
        this: &mut MaybeUninit<Self>,
        init: EspWifiInitialization,
        config: Config,
        wifi: &'static mut Wifi,
        resources: &'static mut StackResources<3>,
        random_seed: u64,
    ) {
        log::info!("Configuring AP");

        let this = this.as_mut_ptr();

        let (wifi_interface, controller) = esp_wifi::wifi::new_with_mode(&init, wifi, WifiMode::Ap);

        unsafe {
            (*this).init = init;
            ptr::write(addr_of_mut!((*this).controller), controller);
            ptr::write(
                addr_of_mut!((*this).stack),
                Stack::new(wifi_interface, config, resources, random_seed),
            );
            ptr::write(
                addr_of_mut!((*this).connection_task_control),
                TaskController::new(),
            );
            ptr::write(
                addr_of_mut!((*this).net_task_control),
                TaskController::new(),
            );
            ptr::write(addr_of_mut!((*this).client_count), Mutex::new(0));
            (*this).started = false;
        }
    }

    pub(super) async fn start(&mut self) -> &mut Stack<WifiDevice<'static>> {
        if !self.started {
            log::info!("Starting AP");
            let spawner = Spawner::for_current_executor().await;
            unsafe {
                log::info!("Starting AP task");
                spawner.must_spawn(ap_task(
                    as_static_mut(&mut self.controller),
                    as_static_ref(&self.connection_task_control),
                    as_static_ref(&self.client_count),
                ));
                log::info!("Starting NET task");
                spawner.must_spawn(net_task(
                    as_static_ref(&self.stack),
                    as_static_ref(&self.net_task_control),
                ));
            }
            self.started = true;
        }

        &mut self.stack
    }

    pub(super) async fn stop(&mut self) {
        if self.started {
            log::info!("Stopping AP");
            let _ = join(
                self.connection_task_control.stop_from_outside(),
                self.net_task_control.stop_from_outside(),
            )
            .await;

            if matches!(self.controller.is_started(), Ok(true)) {
                self.controller.stop().await.unwrap();
            }

            log::info!("Stopped AP");
            self.started = false;
        }
    }

    pub(super) fn is_running(&self) -> bool {
        !self.connection_task_control.has_exited() && !self.net_task_control.has_exited()
    }

    pub(super) async fn client_count(&self) -> u32 {
        *self.client_count.lock().await
    }
}

#[embassy_executor::task]
pub(super) async fn ap_task(
    controller: &'static mut WifiController<'static>,
    task_control: &'static TaskController<()>,
    client_count: &'static Mutex<NoopRawMutex, u32>,
) {
    task_control
        .run_cancellable(async {
            log::info!("Start connection task");
            log::debug!("Device capabilities: {:?}", controller.get_capabilities());

            loop {
                if !matches!(controller.is_started(), Ok(true)) {
                    let client_config = Configuration::AccessPoint(AccessPointConfiguration {
                        ssid: "Card/IO".into(),
                        max_connections: 1,
                        ..Default::default()
                    });
                    controller.set_configuration(&client_config).unwrap();
                    log::info!("Starting wifi");

                    controller.start().await.unwrap();
                    log::info!("Wifi started!");
                }

                if let WifiState::ApStart
                | WifiState::ApStaConnected
                | WifiState::ApStaDisconnected = esp_wifi::wifi::get_wifi_state()
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
                        let count = {
                            let mut count = client_count.lock().await;
                            *count = count.saturating_add(1);
                            *count
                        };
                        log::info!("Client connected, {count} total");
                    }
                    if events.contains(WifiEvent::ApStadisconnected) {
                        let count = {
                            let mut count = client_count.lock().await;
                            *count = count.saturating_sub(1);
                            *count
                        };
                        log::info!("Client disconnected, {count} left");
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
