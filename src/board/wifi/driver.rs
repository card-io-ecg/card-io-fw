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
use embassy_time::{Duration, Timer};
use embedded_svc::wifi::{AccessPointConfiguration, Configuration, Wifi as _};
use esp_wifi::{
    wifi::{WifiController, WifiDevice, WifiEvent, WifiMode, WifiState},
    EspWifiInitFor, EspWifiInitialization,
};
use replace_with::replace_with_or_abort;

pub unsafe fn as_static_ref<T>(what: &T) -> &'static T {
    core::mem::transmute(what)
}

pub unsafe fn as_static_mut<T>(what: &mut T) -> &'static mut T {
    core::mem::transmute(what)
}

// These are static mut because they are not Sync and we don't want to have to implement Sync for
// TaskController. We also don't need these to be Sync because the same executor will be used for
// the tasks.
pub static mut CONNECTION_TASK_CONTROL: TaskController<()> = TaskController::new();
pub static mut NET_TASK_CONTROL: TaskController<()> = TaskController::new();

pub struct WifiDriver {
    wifi: Wifi,
    state: WifiDriverState,
}

#[allow(clippy::large_enum_variant)]
enum WifiDriverState {
    Uninitialized {
        timer: TIMG1,
        rng: RNG,
        rcc: RadioClockControl,
    },
    Initialized {
        init: EspWifiInitialization,
    },
    AP {
        _init: EspWifiInitialization,
        stack: Stack<WifiDevice<'static>>,
    },
}

impl WifiDriver {
    pub fn new(wifi: Wifi, timer: TIMG1, rng: RNG, rcc: RadioClockControl) -> Self {
        Self {
            wifi,
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
                WifiDriverState::Initialized { init } => {
                    let (wifi_interface, mut controller) = esp_wifi::wifi::new_with_mode(
                        &init,
                        unsafe { as_static_mut(&mut self.wifi) },
                        WifiMode::Ap,
                    );

                    let spawner = Spawner::for_current_executor().await;

                    *resources = StackResources::new();
                    let stack = Stack::new(wifi_interface, config, resources, 1234);

                    unsafe {
                        spawner.must_spawn(ap_task(as_static_mut(&mut controller)));
                        spawner.must_spawn(net_task(as_static_ref(&stack)));
                    }

                    WifiDriverState::AP { stack, _init: init }
                }
                WifiDriverState::AP { .. } => this,
            }
        })
        .await;

        match &mut self.state {
            WifiDriverState::Uninitialized { .. } | WifiDriverState::Initialized { .. } => {
                unreachable!()
            }
            WifiDriverState::AP { stack, .. } => stack,
        }
    }

    pub fn initialize(&mut self, clocks: &Clocks, pcc: &mut PeripheralClockControl) {
        replace_with_or_abort(&mut self.state, |this| match this {
            WifiDriverState::Uninitialized { timer, rng, rcc } => {
                let timer = TimerGroup::new(timer, clocks, pcc).timer0;

                let init =
                    esp_wifi::initialize(EspWifiInitFor::Wifi, timer, Rng::new(rng), rcc, clocks)
                        .unwrap();

                WifiDriverState::Initialized { init }
            }
            _ => this,
        })
    }

    pub async fn stop_ap(&mut self) {
        unsafe {
            let _ = join(
                CONNECTION_TASK_CONTROL.stop_from_outside(),
                NET_TASK_CONTROL.stop_from_outside(),
            )
            .await;
        }
    }
}

#[embassy_executor::task]
pub async fn net_task(stack: &'static Stack<WifiDevice<'static>>) {
    unsafe { &NET_TASK_CONTROL }
        .run_cancellable(async {
            stack.run().await;
        })
        .await;
}

#[embassy_executor::task]
pub async fn ap_task(controller: &'static mut WifiController<'static>) {
    unsafe { &CONNECTION_TASK_CONTROL }
        .run_cancellable(async {
            log::debug!("start connection task");
            log::debug!("Device capabilities: {:?}", controller.get_capabilities());

            loop {
                if let WifiState::ApStart = esp_wifi::wifi::get_wifi_state() {
                    // wait until we're no longer connected
                    controller.wait_for_event(WifiEvent::ApStop).await;
                    Timer::after(Duration::from_millis(5000)).await;

                    // TODO: exit app state if disconnected?
                }

                if !matches!(controller.is_started(), Ok(true)) {
                    let client_config = Configuration::AccessPoint(AccessPointConfiguration {
                        ssid: "Card/IO".into(),
                        ..Default::default()
                    });
                    controller.set_configuration(&client_config).unwrap();
                    log::debug!("Starting wifi");

                    controller.start().await.unwrap();
                    log::debug!("Wifi started!");
                }
            }
        })
        .await;
}
