use embassy_executor::SendSpawner;
use embedded_hal::digital::InputPin;
use gui::screens::BatteryInfo;

use crate::{
    board::{
        hal::{clock::Clocks, radio::Wifi},
        ChargerStatus, Config, EcgFrontend, PoweredDisplay, VbusDetect,
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

pub struct Board {
    pub display: PoweredDisplay,
    pub frontend: EcgFrontend,
    pub clocks: Clocks<'static>,
    pub high_prio_spawner: SendSpawner,
    pub battery_monitor: BatteryMonitor<VbusDetect, ChargerStatus>,
    pub wifi: Wifi,
    pub config: Config,
}
