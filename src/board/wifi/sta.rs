use crate::{
    board::{
        hal::radio::Wifi,
        wifi::{as_static_mut, as_static_ref, net_task},
    },
    task_control::TaskController,
};
use config_site::data::network::WifiNetwork;
use embassy_executor::Spawner;
use embassy_futures::join::join;
use embassy_net::{Config, Stack, StackResources};
use embassy_time::{Duration, Timer};
use embedded_svc::wifi::{ClientConfiguration, Configuration, Wifi as _};
use esp_wifi::{
    wifi::{WifiController, WifiDevice, WifiEvent, WifiMode},
    EspWifiInitialization,
};

pub(super) struct StaState {
    init: EspWifiInitialization,
    controller: WifiController<'static>,
    stack: Stack<WifiDevice<'static>>,
    connection_task_control: TaskController<()>,
    net_task_control: TaskController<!>,
    started: bool,
}

impl StaState {
    pub(super) fn new(
        init: EspWifiInitialization,
        config: Config,
        wifi: &'static mut Wifi,
        resources: &'static mut StackResources<3>,
        random_seed: u64,
    ) -> Self {
        let (wifi_interface, controller) =
            esp_wifi::wifi::new_with_mode(&init, wifi, WifiMode::Sta);

        Self {
            init,
            controller,
            stack: Stack::new(wifi_interface, config, resources, random_seed),
            connection_task_control: TaskController::new(),
            net_task_control: TaskController::new(),
            started: false,
        }
    }

    pub(super) async fn deinit(mut self) -> EspWifiInitialization {
        self.stop().await;
        self.init
    }

    pub(super) async fn stop(&mut self) {
        if self.started {
            let _ = join(
                self.connection_task_control.stop_from_outside(),
                self.net_task_control.stop_from_outside(),
            )
            .await;
            self.started = false;
        }
    }

    pub(super) async fn start(&mut self) -> &mut Stack<WifiDevice<'static>> {
        if !self.started {
            let spawner = Spawner::for_current_executor().await;
            unsafe {
                spawner.must_spawn(sta_task(
                    as_static_mut(&mut self.controller),
                    as_static_ref(&self.connection_task_control),
                ));
                spawner.must_spawn(net_task(
                    as_static_ref(&self.stack),
                    as_static_ref(&self.net_task_control),
                ));
            }
            self.started = true;
        }

        &mut self.stack
    }

    pub(super) fn is_connected(&self) -> bool {
        false
    }
}

#[embassy_executor::task]
pub(super) async fn sta_task(
    controller: &'static mut WifiController<'static>,
    task_control: &'static TaskController<()>,
) {
    task_control
        .run_cancellable(async {
            let known_networks = [];

            let connect_to = 'select: loop {
                if let Some(connect_to) =
                    select_visible_known_network(controller, &known_networks).await
                {
                    break 'select connect_to;
                }

                Timer::after(Duration::from_secs(5)).await;
            };

            if !matches!(controller.is_started(), Ok(true)) {
                controller
                    .set_configuration(&Configuration::Client(ClientConfiguration {
                        ssid: known_networks[connect_to].ssid.clone(),
                        password: known_networks[connect_to].pass.clone(),
                        ..Default::default()
                    }))
                    .unwrap();
                log::info!("Starting wifi");
                controller.start().await.unwrap();
                log::info!("Wifi started!");
            }

            log::info!("Connecting...");

            match controller.connect().await {
                Ok(_) => log::info!("Wifi connected!"),
                Err(e) => {
                    log::warn!("Failed to connect to wifi: {e:?}");
                    Timer::after(Duration::from_millis(5000)).await
                }
            }

            controller.wait_for_event(WifiEvent::StaDisconnected).await;
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
