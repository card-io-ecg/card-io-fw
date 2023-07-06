use crate::{
    board::hal::{
        clock::Clocks,
        peripherals::{RNG, TIMG1},
        radio::Wifi,
        system::{PeripheralClockControl, RadioClockControl},
        timer::TimerGroup,
        Rng,
    },
    replace_with::replace_with_or_abort_async,
    task_control::TaskController,
};
use embassy_executor::Spawner;
use embassy_futures::join::join;
use embassy_net::{Config, Stack, StackResources};
use embassy_sync::{blocking_mutex::raw::NoopRawMutex, mutex::Mutex};
use embassy_time::{Duration, Timer};
use embedded_hal_old::prelude::_embedded_hal_blocking_rng_Read;
use embedded_svc::wifi::{AccessPointConfiguration, Configuration, Wifi as _};
use esp_wifi::{
    wifi::{WifiController, WifiDevice, WifiEvent, WifiMode, WifiState},
    EspWifiInitFor, EspWifiInitialization,
};
use rand_core::{RngCore, SeedableRng};
use replace_with::replace_with_or_abort;
use wyhash::WyRng;

pub unsafe fn as_static_ref<T>(what: &T) -> &'static T {
    core::mem::transmute(what)
}

pub unsafe fn as_static_mut<T>(what: &mut T) -> &'static mut T {
    core::mem::transmute(what)
}

pub struct WifiDriver {
    wifi: Wifi,
    rng: WyRng,
    state: WifiDriverState,
}

struct ApState {
    _init: EspWifiInitialization,
    controller: WifiController<'static>,
    stack: Stack<WifiDevice<'static>>,
    connection_task_control: TaskController<()>,
    net_task_control: TaskController<()>,
    client_count: Mutex<NoopRawMutex, u32>,
    started: bool,
}

impl ApState {
    fn new(
        _init: EspWifiInitialization,
        config: Config,
        wifi: &'static mut Wifi,
        resources: &'static mut StackResources<3>,
        random_seed: u64,
    ) -> Self {
        let (wifi_interface, controller) =
            esp_wifi::wifi::new_with_mode(&_init, wifi, WifiMode::Ap);

        Self {
            _init,
            controller,
            stack: Stack::new(wifi_interface, config, resources, random_seed),
            connection_task_control: TaskController::new(),
            net_task_control: TaskController::new(),
            client_count: Mutex::new(0),
            started: false,
        }
    }

    async fn start(&mut self) -> &mut Stack<WifiDevice<'static>> {
        if !self.started {
            let spawner = Spawner::for_current_executor().await;
            unsafe {
                spawner.must_spawn(ap_task(
                    as_static_mut(&mut self.controller),
                    as_static_ref(&self.connection_task_control),
                    as_static_ref(&self.client_count),
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

    async fn stop(&mut self) {
        if self.started {
            let _ = join(
                self.connection_task_control.stop_from_outside(),
                self.net_task_control.stop_from_outside(),
            )
            .await;
            self.started = false;
        }
    }

    fn is_running(&self) -> bool {
        !self.connection_task_control.has_exited() && !self.net_task_control.has_exited()
    }

    async fn client_count(&self) -> u32 {
        *self.client_count.lock().await
    }
}

#[allow(clippy::large_enum_variant)]
enum WifiDriverState {
    Uninitialized {
        timer: TIMG1,
        rng: Rng<'static>,
        rcc: RadioClockControl,
    },
    Initialized {
        init: EspWifiInitialization,
    },
    AP(ApState),
}

impl WifiDriver {
    pub fn new(wifi: Wifi, timer: TIMG1, rng: RNG, rcc: RadioClockControl) -> Self {
        let mut rng = Rng::new(rng);
        let mut seed_bytes = [0; 8];
        rng.read(&mut seed_bytes).unwrap();
        Self {
            wifi,
            rng: WyRng::from_seed(seed_bytes),
            state: WifiDriverState::Uninitialized { timer, rng, rcc },
        }
    }

    pub async fn configure_ap<'d>(
        &'d mut self,
        config: Config,
        resources: &'static mut StackResources<3>,
    ) -> &'d mut Stack<WifiDevice<'static>> {
        replace_with_or_abort_async(&mut self.state, |this| async {
            match this {
                WifiDriverState::Uninitialized { .. } => unreachable!(),
                WifiDriverState::Initialized { init } => WifiDriverState::AP(ApState::new(
                    init,
                    config,
                    unsafe { as_static_mut(&mut self.wifi) },
                    resources,
                    self.rng.next_u64(),
                )),
                WifiDriverState::AP { .. } => this,
            }
        })
        .await;

        match &mut self.state {
            WifiDriverState::AP(ap) => ap.start().await,

            WifiDriverState::Uninitialized { .. } | WifiDriverState::Initialized { .. } => {
                unreachable!()
            }
        }
    }

    pub fn initialize(&mut self, clocks: &Clocks, pcc: &mut PeripheralClockControl) {
        replace_with_or_abort(&mut self.state, |this| match this {
            WifiDriverState::Uninitialized { timer, rng, rcc } => {
                let timer = TimerGroup::new(timer, clocks, pcc).timer0;

                let init =
                    esp_wifi::initialize(EspWifiInitFor::Wifi, timer, rng, rcc, clocks).unwrap();

                WifiDriverState::Initialized { init }
            }
            _ => this,
        })
    }

    pub async fn ap_client_count(&self) -> u32 {
        if let WifiDriverState::AP(ap) = &self.state {
            ap.client_count().await
        } else {
            0
        }
    }

    pub async fn stop_ap(&mut self) {
        if let WifiDriverState::AP(ap) = &mut self.state {
            ap.stop().await;
        }
    }

    pub fn ap_running(&self) -> bool {
        if let WifiDriverState::AP(ap) = &self.state {
            ap.is_running()
        } else {
            false
        }
    }
}

#[embassy_executor::task]
pub async fn net_task(
    stack: &'static Stack<WifiDevice<'static>>,
    task_control: &'static TaskController<()>,
) {
    task_control
        .run_cancellable(async {
            stack.run().await;
        })
        .await;
}

#[embassy_executor::task]
pub async fn ap_task(
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
