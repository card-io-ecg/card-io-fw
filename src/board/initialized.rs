use crate::{
    board::{
        self,
        config::Config,
        hal::{clock::Clocks, system::PeripheralClockControl},
        wifi::{ap::Ap, WifiDriver},
        ChargerStatus, EcgFrontend, PoweredDisplay, VbusDetect,
    },
    SharedBatteryState,
};
use embassy_executor::SendSpawner;
use embassy_net::{Config as NetConfig, Ipv4Address, Ipv4Cidr, StaticConfigV4};
use embassy_sync::{blocking_mutex::raw::NoopRawMutex, mutex::Mutex, signal::Signal};
use embedded_hal::digital::InputPin;
use gui::screens::BatteryInfo;
use norfs::{
    drivers::internal::{InternalDriver, InternalPartition},
    medium::cache::ReadCache,
    OnCollision, Storage,
};

#[cfg(feature = "battery_adc")]
use crate::board::drivers::battery_adc::BatteryAdcData;

#[cfg(feature = "battery_max17055")]
use crate::board::drivers::battery_fg::BatteryFgData;

#[cfg(any(feature = "battery_adc", feature = "battery_max17055"))]
use crate::board::LOW_BATTERY_PERCENTAGE;
#[cfg(any(feature = "battery_adc", feature = "battery_max17055"))]
use embassy_executor::Spawner;

use super::wifi::sta::Sta;

#[derive(Default, Clone, Copy)]
pub struct BatteryState {
    #[cfg(feature = "battery_adc")]
    pub adc_data: Option<BatteryAdcData>,
    #[cfg(feature = "battery_max17055")]
    pub fg_data: Option<BatteryFgData>,
}

pub struct BatteryMonitor<VBUS, CHG> {
    pub battery_state: &'static SharedBatteryState,
    pub vbus_detect: VBUS,
    pub charger_status: CHG,
    pub last_battery_state: BatteryState,
    pub signal: &'static Signal<NoopRawMutex, ()>,
}

impl<VBUS: InputPin, CHG: InputPin> BatteryMonitor<VBUS, CHG> {
    #[cfg(feature = "battery_adc")]
    pub async fn start(
        &mut self,
        adc: board::BatteryAdc,
        battery_state: &'static Mutex<NoopRawMutex, BatteryState>,
    ) {
        let spawner = Spawner::for_current_executor().await;
        spawner
            .spawn(board::drivers::battery_adc::monitor_task_adc(
                adc,
                battery_state,
                self.signal,
            ))
            .ok();
    }

    #[cfg(feature = "battery_max17055")]
    pub async fn start(
        &mut self,
        fg: board::BatteryFg,
        battery_state: &'static Mutex<NoopRawMutex, BatteryState>,
    ) {
        let spawner = Spawner::for_current_executor().await;
        spawner
            .spawn(board::drivers::battery_fg::monitor_task_fg(
                fg,
                battery_state,
                self.signal,
            ))
            .ok();
    }

    fn load_battery_data(&mut self) {
        if let Ok(state) = self.battery_state.try_lock() {
            self.last_battery_state = *state;
        }
    }

    #[cfg(feature = "battery_adc")]
    pub fn battery_data(&mut self) -> Option<BatteryInfo> {
        let battery_model = signal_processing::battery::BatteryModel {
            voltage: (2750, 4200),
            charge_current: (0, 1000),
        };

        self.load_battery_data();

        self.last_battery_state.adc_data.map(|state| {
            let charge_current = if self.is_charging() {
                None
            } else {
                Some(state.charge_current)
            };

            let percentage = battery_model.estimate(state.voltage, charge_current);

            BatteryInfo {
                voltage: state.voltage,
                is_charging: self.is_charging(),
                percentage,
                is_low: percentage < LOW_BATTERY_PERCENTAGE,
            }
        })
    }

    #[cfg(feature = "battery_max17055")]
    pub fn battery_data(&mut self) -> Option<BatteryInfo> {
        self.load_battery_data();

        self.last_battery_state.fg_data.map(|state| BatteryInfo {
            voltage: state.voltage,
            is_charging: self.is_charging(),
            percentage: state.percentage,
            is_low: state.percentage < LOW_BATTERY_PERCENTAGE,
        })
    }

    #[cfg(not(any(feature = "battery_max17055", feature = "battery_adc")))]
    pub fn battery_data(&mut self) -> Option<BatteryInfo> {
        None
    }

    pub fn is_plugged(&self) -> bool {
        self.vbus_detect.is_high().unwrap()
    }

    pub fn is_charging(&self) -> bool {
        self.charger_status.is_low().unwrap()
    }

    pub async fn stop(&mut self) {
        self.signal.signal(());
    }
}

pub struct ConfigPartition;
impl InternalPartition for ConfigPartition {
    const OFFSET: usize = 0x410000;
    const SIZE: usize = 4032 * 1024;
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StaMode {
    Enable,
    OnDemand,
}

pub struct Board {
    pub display: PoweredDisplay,
    pub frontend: EcgFrontend,
    pub clocks: Clocks<'static>,
    pub peripheral_clock_control: PeripheralClockControl,
    pub high_prio_spawner: SendSpawner,
    pub battery_monitor: BatteryMonitor<VbusDetect, ChargerStatus>,
    pub wifi: &'static mut WifiDriver,
    pub config: &'static mut Config,
    pub config_changed: bool,
    pub storage: Option<
        &'static mut Storage<&'static mut ReadCache<InternalDriver<ConfigPartition>, 256, 2>>,
    >,
}

impl Board {
    pub async fn save_config(&mut self) {
        if !self.config_changed {
            return;
        }

        log::info!("Saving config");
        self.config_changed = false;

        if let Some(storage) = self.storage.as_mut() {
            if let Err(e) = storage
                .store_writer("config", self.config, OnCollision::Overwrite)
                .await
            {
                log::error!("Failed to save config: {e:?}");
            }
        } else {
            log::warn!("Storage unavailable");
        }
    }

    pub async fn enable_wifi_sta(&mut self, mode: StaMode) -> Option<Sta> {
        let can_enable = self.can_enable_wifi()
            && match mode {
                StaMode::Enable => true,
                StaMode::OnDemand => self.sta_has_work(),
            };

        if !can_enable {
            self.wifi.stop_if().await;
            return None;
        }

        self.wifi.initialize(&self.clocks);

        let sta = self
            .wifi
            .configure_sta(NetConfig::dhcpv4(Default::default()))
            .await;

        sta.update_known_networks(&self.config.known_networks).await;

        Some(sta)
    }

    pub async fn enable_wifi_ap(&mut self) -> Option<Ap> {
        if !self.can_enable_wifi() {
            self.wifi.stop_if().await;
            return None;
        }

        self.wifi.initialize(&self.clocks);

        let ap = self
            .wifi
            .configure_ap(NetConfig::ipv4_static(StaticConfigV4 {
                address: Ipv4Cidr::new(Ipv4Address::new(192, 168, 2, 1), 24),
                gateway: Some(Ipv4Address::from_bytes(&[192, 168, 2, 1])),
                dns_servers: Default::default(),
            }))
            .await;

        Some(ap)
    }

    /// Note: make sure Sta/Ap is released before calling this.
    pub async fn disable_wifi(&mut self) {
        self.wifi.stop_if().await
    }

    pub fn can_enable_wifi(&mut self) -> bool {
        self.battery_monitor
            .battery_data()
            .map(|battery| battery.percentage > 50 || battery.is_charging)
            .unwrap_or(false)
    }

    fn sta_has_work(&self) -> bool {
        // TODO: we can do a flag that is true on boot, so that entering the menu will always
        // connect and look for update, etc. We can also use a flag to see if we have ongoing
        // communication, so we can keep wifi on. Question is: when/how do we disable wifi if
        // it is in on-demand mode?
        false
    }
}
