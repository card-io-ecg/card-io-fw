use core::{
    mem::MaybeUninit,
    ptr::{self, addr_of_mut},
};

use crate::{
    board::{
        hal::{radio::Wifi, Rng},
        wifi::net_task,
    },
    task_control::{TaskControlToken, TaskController},
};
use alloc::rc::Rc;
use config_site::data::network::WifiNetwork;
use embassy_executor::Spawner;
use embassy_futures::join::join;
use embassy_net::{Config, Stack, StackResources};
use embassy_sync::{blocking_mutex::raw::NoopRawMutex, mutex::Mutex};
use embassy_time::{Duration, Timer};
use embedded_hal_old::prelude::_embedded_hal_blocking_rng_Read;
use embedded_svc::wifi::{ClientConfiguration, Configuration, Wifi as _};
use esp_wifi::{
    wifi::{WifiController, WifiDevice, WifiEvent, WifiMode},
    EspWifiInitialization,
};

#[derive(Clone)]
pub struct Sta {
    stack: Rc<Stack<WifiDevice<'static>>>,
}

impl Sta {
    pub fn stack(&self) -> &Stack<WifiDevice<'static>> {
        &self.stack
    }
}

pub(super) struct StaState {
    init: EspWifiInitialization,
    controller: Rc<Mutex<NoopRawMutex, WifiController<'static>>>,
    stack: Rc<Stack<WifiDevice<'static>>>,
    connection_task_control: TaskController<()>,
    net_task_control: TaskController<!>,
    started: bool,
}

impl StaState {
    pub(super) fn init(
        this: &mut MaybeUninit<Self>,
        init: EspWifiInitialization,
        config: Config,
        wifi: &'static mut Wifi,
        resources: &'static mut StackResources<3>,
        mut rng: Rng,
    ) {
        log::info!("Configuring STA");

        let this = this.as_mut_ptr();

        let (wifi_interface, controller) =
            esp_wifi::wifi::new_with_mode(&init, wifi, WifiMode::Sta).unwrap();

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
            (*this).started = false;
        }
    }

    pub(super) fn unwrap(self) -> EspWifiInitialization {
        self.init
    }

    pub(super) async fn stop(&mut self) {
        if self.started {
            log::info!("Stopping STA");
            let _ = join(
                self.connection_task_control.stop_from_outside(),
                self.net_task_control.stop_from_outside(),
            )
            .await;

            if matches!(self.controller.lock().await.is_started(), Ok(true)) {
                self.controller.lock().await.stop().await.unwrap();
            }

            log::info!("Stopped STA");
            self.started = false;
        }
    }

    pub(super) async fn start(&mut self) -> Sta {
        if !self.started {
            log::info!("Starting STA");
            let spawner = Spawner::for_current_executor().await;

            log::info!("Starting STA task");
            spawner.must_spawn(sta_task(
                self.controller.clone(),
                self.connection_task_control.token(),
            ));
            log::info!("Starting NET task");
            spawner.must_spawn(net_task(self.stack.clone(), self.net_task_control.token()));

            self.started = true;
        }

        Sta {
            stack: self.stack.clone(),
        }
    }

    pub(super) fn is_connected(&self) -> bool {
        false
    }
}

#[embassy_executor::task]
pub(super) async fn sta_task(
    controller: Rc<Mutex<NoopRawMutex, WifiController<'static>>>,
    mut task_control: TaskControlToken<()>,
) {
    task_control
        .run_cancellable(async {
            let known_networks = [];

            loop {
                if !matches!(controller.lock().await.is_started(), Ok(true)) {
                    log::info!("Starting wifi");
                    controller.lock().await.start().await.unwrap();
                    log::info!("Wifi started!");
                }

                let connect_to = 'select: loop {
                    let mut controller = controller.lock().await;
                    if let Some(connect_to) =
                        select_visible_known_network(&mut controller, &known_networks).await
                    {
                        break 'select connect_to;
                    }
                    core::mem::drop(controller);

                    Timer::after(Duration::from_secs(5)).await;
                };

                controller
                    .lock()
                    .await
                    .set_configuration(&Configuration::Client(ClientConfiguration {
                        ssid: known_networks[connect_to].ssid.clone(),
                        password: known_networks[connect_to].pass.clone(),
                        ..Default::default()
                    }))
                    .unwrap();

                log::info!("Connecting...");

                match controller.lock().await.connect().await {
                    Ok(_) => log::info!("Wifi connected!"),
                    Err(e) => {
                        log::warn!("Failed to connect to wifi: {e:?}");
                        Timer::after(Duration::from_millis(5000)).await
                    }
                }

                controller
                    .lock()
                    .await
                    .wait_for_event(WifiEvent::StaDisconnected)
                    .await;
            }
        })
        .await;
}

async fn select_visible_known_network(
    controller: &mut WifiController<'static>,
    known_networks: &[WifiNetwork],
) -> Option<usize> {
    match controller.scan_n::<8>().await {
        Ok((mut networks, _)) => {
            // Sort by signal strength, desc
            networks.sort_by(|a, b| b.signal_strength.cmp(&a.signal_strength));
            for network in networks {
                if let Some(pos) = known_networks.iter().position(|n| n.ssid == network.ssid) {
                    return Some(pos);
                }
            }
        }
        Err(err) => log::warn!("Scan failed: {err:?}"),
    }
    None
}
