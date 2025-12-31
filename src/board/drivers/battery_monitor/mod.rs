use core::ops::{Deref, DerefMut};

use crate::{task_control::TaskController, Shared, SharedGuard};
use alloc::rc::Rc;
use embassy_sync::mutex::Mutex;
use embedded_hal::digital::InputPin;
use gui::screens::{BatteryInfo, ChargingState};

#[cfg(feature = "battery_max17055")]
pub mod battery_fg;

#[cfg(feature = "battery_max17055")]
use crate::board::{
    drivers::battery_monitor::battery_fg::{
        monitor_task_fg as monitor_task, BatteryFgData as BatteryData,
    },
    BatteryFg as BatterySensorImpl,
};

#[cfg(feature = "battery_max17055")]
use crate::board::LOW_BATTERY_PERCENTAGE;

#[cfg(feature = "battery_max17055")]
use embassy_executor::Spawner;

#[derive(Default, Clone, Copy)]
struct BatteryState {
    data: Option<BatteryData>,
}

pub struct BatterySensor {
    state: BatteryState,
    sensor: BatterySensorImpl,
}

impl BatterySensor {
    pub fn update_data(&mut self, data: BatteryData) {
        self.state.data = Some(data);
    }
}

impl Deref for BatterySensor {
    type Target = BatterySensorImpl;

    fn deref(&self) -> &Self::Target {
        &self.sensor
    }
}

impl DerefMut for BatterySensor {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.sensor
    }
}

pub struct BatteryMonitor<VBUS, CHG> {
    vbus_detect: VBUS,
    charger_status: CHG,
    last_battery_state: BatteryState,
    signal: TaskController<()>,
    sensor: Shared<BatterySensor>,
}

impl<VBUS, CHG> BatteryMonitor<VBUS, CHG>
where
    VBUS: InputPin,
    CHG: InputPin,
{
    pub async fn start(vbus_detect: VBUS, charger_status: CHG, sensor: BatterySensorImpl) -> Self {
        let this = BatteryMonitor {
            sensor: Rc::new(Mutex::new(BatterySensor {
                state: BatteryState::default(),
                sensor,
            })),
            vbus_detect,
            charger_status,
            last_battery_state: BatteryState::default(),
            signal: TaskController::new(),
        };

        let spawner = unsafe { Spawner::for_current_executor().await };
        spawner
            .spawn(monitor_task(this.sensor.clone(), this.signal.token()))
            .ok();

        this
    }

    fn load_battery_data(&mut self) {
        if let Ok(state) = self.sensor.try_lock() {
            self.last_battery_state = state.state;
        }
    }

    pub fn battery_data(&mut self) -> Option<BatteryInfo> {
        self.load_battery_data();
        self.last_battery_data()
    }

    fn last_battery_data(&mut self) -> Option<BatteryInfo> {
        self.last_battery_state
            .data
            .map(|data| self.convert_battery_data(data))
    }

    pub fn charging_state(&mut self) -> ChargingState {
        match (self.is_plugged(), self.is_charging()) {
            (_, true) => ChargingState::Charging,
            (true, false) => ChargingState::Plugged,
            (false, false) => ChargingState::Discharging,
        }
    }

    pub fn is_plugged(&mut self) -> bool {
        unwrap!(self.vbus_detect.is_high().ok())
    }

    pub fn is_charging(&mut self) -> bool {
        unwrap!(self.charger_status.is_low().ok())
    }

    pub fn is_low(&mut self) -> bool {
        self.last_battery_data()
            .map(|data| data.is_low)
            .unwrap_or(false)
    }

    pub async fn sensor(&self) -> SharedGuard<'_, BatterySensor> {
        self.sensor.lock().await
    }

    pub async fn stop(self) -> (VBUS, CHG) {
        _ = self.signal.stop().await;
        (self.vbus_detect, self.charger_status)
    }
}

#[cfg(feature = "battery_max17055")]
impl<VBUS, CHG> BatteryMonitor<VBUS, CHG>
where
    VBUS: InputPin,
    CHG: InputPin,
{
    pub fn convert_battery_data(&mut self, data: BatteryData) -> BatteryInfo {
        BatteryInfo {
            voltage: data.voltage,
            charging_state: self.charging_state(),
            percentage: data.percentage,
            is_low: data.percentage < LOW_BATTERY_PERCENTAGE,
        }
    }
}
