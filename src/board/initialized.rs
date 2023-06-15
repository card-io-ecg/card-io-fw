use embassy_executor::SendSpawner;
use embedded_hal::digital::InputPin;
use esp32s3_hal::system::PeripheralClockControl;
use gui::screens::BatteryInfo;
use storage::{
    drivers::internal::{InternalDriver, InternalPartition},
    medium::cache::ReadCache,
    OnCollision, Storage,
};

use crate::{
    board::{
        config::{Config, ConfigFile},
        hal::clock::Clocks,
        wifi_driver::WifiDriver,
        ChargerStatus, EcgFrontend, PoweredDisplay, VbusDetect,
    },
    SharedBatteryState,
};

pub struct BatteryMonitor<VBUS, CHG> {
    pub battery_state: &'static SharedBatteryState,
    pub vbus_detect: VBUS,
    pub charger_status: CHG,
}

impl<VBUS: InputPin, CHG: InputPin> BatteryMonitor<VBUS, CHG> {
    pub async fn battery_data(&mut self) -> Option<BatteryInfo> {
        let state = self.battery_state.lock().await;
        let battery_voltage = state.battery_voltage;
        let charge_current = state.charging_current;

        let is_plugged = self.is_plugged();

        battery_voltage.map(|voltage| BatteryInfo {
            voltage,
            charge_current: if is_plugged { charge_current } else { None },
        })
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
    pub wifi: WifiDriver,
    pub config: Config,
    pub config_changed: bool,
    pub storage: Option<Storage<ReadCache<InternalDriver<ConfigPartition>, 256, 2>>>,
}

impl Board {
    pub async fn save_config(&mut self) {
        if !self.config_changed {
            return;
        }

        log::info!("Saving config");
        self.config_changed = false;

        if let Some(storage) = self.storage.as_mut() {
            let config_data = ConfigFile::new(self.config);

            let serialized_config = config_data.into_vec();
            if let Err(e) = storage
                .store("config", &serialized_config, OnCollision::Overwrite)
                .await
            {
                log::error!("Failed to save config: {:?}", e);
            }
        } else {
            log::warn!("Storage unavailable");
        }
    }
}
