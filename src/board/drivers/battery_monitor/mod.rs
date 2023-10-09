use crate::{task_control::TaskController, Shared, SharedGuard};
use alloc::rc::Rc;
use embassy_sync::mutex::Mutex;
use embedded_hal::digital::InputPin;
use gui::screens::{BatteryInfo, ChargingState};

#[cfg(feature = "battery_adc")]
pub mod battery_adc;
#[cfg(feature = "battery_max17055")]
pub mod battery_fg;

#[cfg(feature = "battery_adc")]
use crate::board::{
    drivers::battery_monitor::battery_adc::{
        monitor_task_adc as monitor_task, BatteryAdcData as BatteryData,
    },
    BatteryAdc as BatterySensor,
};

#[cfg(feature = "battery_max17055")]
use crate::board::{
    drivers::battery_monitor::battery_fg::{
        monitor_task_fg as monitor_task, BatteryFgData as BatteryData,
    },
    BatteryFg as BatterySensor,
};

#[cfg(any(feature = "battery_adc", feature = "battery_max17055"))]
use crate::board::LOW_BATTERY_PERCENTAGE;

#[cfg(any(feature = "battery_adc", feature = "battery_max17055"))]
use embassy_executor::Spawner;

#[derive(Default, Clone, Copy)]
pub struct BatteryState {
    pub data: Option<BatteryData>,
}

type SharedBatteryState = Shared<BatteryState>;

pub struct BatteryMonitor<VBUS, CHG> {
    battery_state: SharedBatteryState,
    vbus_detect: VBUS,
    charger_status: CHG,
    last_battery_state: BatteryState,
    signal: TaskController<()>,
    sensor: Shared<BatterySensor>,
}

impl<VBUS: InputPin, CHG: InputPin> BatteryMonitor<VBUS, CHG> {
    pub async fn start(vbus_detect: VBUS, charger_status: CHG, adc: BatterySensor) -> Self {
        let this = BatteryMonitor {
            battery_state: Rc::new(Mutex::new(BatteryState::default())),
            sensor: Rc::new(Mutex::new(adc)),
            vbus_detect,
            charger_status,
            last_battery_state: BatteryState::default(),
            signal: TaskController::new(),
        };

        let spawner = Spawner::for_current_executor().await;
        spawner
            .spawn(monitor_task(
                this.sensor.clone(),
                this.battery_state.clone(),
                this.signal.token(),
            ))
            .ok();

        this
    }

    fn load_battery_data(&mut self) {
        if let Ok(state) = self.battery_state.try_lock() {
            self.last_battery_state = *state;
        }
    }

    pub fn battery_data(&mut self) -> Option<BatteryInfo> {
        self.load_battery_data();
        self.last_battery_data()
    }

    fn last_battery_data(&self) -> Option<BatteryInfo> {
        self.last_battery_state
            .data
            .map(|data| self.convert_battery_data(data))
    }

    pub fn charging_state(&self) -> ChargingState {
        match (self.is_plugged(), self.is_charging()) {
            (_, true) => ChargingState::Charging,
            (true, false) => ChargingState::Plugged,
            (false, false) => ChargingState::Discharging,
        }
    }

    pub fn is_plugged(&self) -> bool {
        unwrap!(self.vbus_detect.is_high().ok())
    }

    pub fn is_charging(&self) -> bool {
        unwrap!(self.charger_status.is_low().ok())
    }

    pub fn is_low(&self) -> bool {
        self.last_battery_data()
            .map(|data| data.is_low)
            .unwrap_or(false)
    }

    #[allow(unused)]
    pub async fn sensor(&self) -> SharedGuard<'_, BatterySensor> {
        self.sensor.lock().await
    }

    pub async fn stop(self) -> (VBUS, CHG) {
        _ = self.signal.stop().await;
        (self.vbus_detect, self.charger_status)
    }
}

#[cfg(feature = "battery_adc")]
impl<VBUS: InputPin, CHG: InputPin> BatteryMonitor<VBUS, CHG> {
    fn convert_battery_data(&self, data: BatteryData) -> BatteryInfo {
        use signal_processing::battery::BatteryModel;

        let battery_model = BatteryModel {
            voltage: (2750, 4200),
            charge_current: (0, 1000),
        };

        let charge_current = if self.is_charging() {
            None
        } else {
            Some(data.charge_current)
        };

        let percentage = battery_model.estimate(data.voltage, charge_current);

        BatteryInfo {
            voltage: data.voltage,
            charging_state: self.charging_state(),
            percentage,
            is_low: percentage < LOW_BATTERY_PERCENTAGE,
        }
    }
}

#[cfg(feature = "battery_max17055")]
impl<VBUS: InputPin, CHG: InputPin> BatteryMonitor<VBUS, CHG> {
    pub fn convert_battery_data(&self, data: BatteryData) -> BatteryInfo {
        BatteryInfo {
            voltage: data.voltage,
            charging_state: self.charging_state(),
            percentage: data.percentage,
            is_low: data.percentage < LOW_BATTERY_PERCENTAGE,
        }
    }
}
