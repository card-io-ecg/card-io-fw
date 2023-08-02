use crate::{
    board::{
        config::{Config, ConfigFile},
        hal::{clock::Clocks, system::PeripheralClockControl},
        wifi::WifiDriver,
        ChargerStatus, EcgFrontend, PoweredDisplay, VbusDetect,
    },
    SharedBatteryState,
};
use embassy_executor::SendSpawner;
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
}

impl<VBUS: InputPin, CHG: InputPin> BatteryMonitor<VBUS, CHG> {
    #[cfg(feature = "battery_adc")]
    pub async fn battery_data(&mut self) -> Option<BatteryInfo> {
        let battery_model = signal_processing::battery::BatteryModel {
            voltage: (2750, 4200),
            charge_current: (0, 1000),
        };

        let state = self.battery_state.lock().await;

        state.adc_data.map(|state| {
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
    pub async fn battery_data(&mut self) -> Option<BatteryInfo> {
        let state = self.battery_state.lock().await;

        state.fg_data.map(|state| BatteryInfo {
            voltage: state.voltage,
            is_charging: self.is_charging(),
            percentage: state.percentage,
            is_low: state.percentage < LOW_BATTERY_PERCENTAGE,
        })
    }

    #[cfg(not(any(feature = "battery_max17055", feature = "battery_adc")))]
    pub async fn battery_data(&mut self) -> Option<BatteryInfo> {
        None
    }

    pub fn is_plugged(&self) -> bool {
        self.vbus_detect.is_high().unwrap()
    }

    pub fn is_charging(&self) -> bool {
        self.charger_status.is_low().unwrap()
    }
}

pub struct ConfigPartition;
impl InternalPartition for ConfigPartition {
    const OFFSET: usize = 0x410000;
    const SIZE: usize = 4032 * 1024;
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
            let config_data = ConfigFile::new(self.config.clone());

            if let Err(e) = storage
                .store_writer("config", &config_data, OnCollision::Overwrite)
                .await
            {
                log::error!("Failed to save config: {e:?}");
            }
        } else {
            log::warn!("Storage unavailable");
        }
    }
}
