use core::ops::{Deref, DerefMut};

use crate::{
    board::startup::BatterySensorDriver, task_control::TaskController, Shared, SharedGuard,
};
use alloc::rc::Rc;
use embassy_sync::mutex::Mutex;
use embassy_time::{Duration, Instant};
use embedded_hal::digital::InputPin;
use gui::screens::{BatteryInfo, ChargingState};
use signal_processing::debounce::Debouncer;

pub mod battery_fg;

use crate::board::drivers::battery_monitor::battery_fg::{
    monitor_task_fg as monitor_task, BatteryFgData as BatteryData,
};

use config_types::LOW_BATTERY_PERCENTAGE;

use embassy_executor::Spawner;

#[derive(Default, Clone, Copy)]
struct BatteryState {
    data: Option<BatteryData>,
}

pub struct BatterySensor {
    state: BatteryState,
    sensor: BatterySensorDriver,
}

impl BatterySensor {
    pub fn update_data(&mut self, data: BatteryData) {
        self.state.data = Some(data);
    }
}

impl Deref for BatterySensor {
    type Target = BatterySensorDriver;

    fn deref(&self) -> &Self::Target {
        &self.sensor
    }
}

impl DerefMut for BatterySensor {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.sensor
    }
}

/// A charger pin, which reports a new level only after it held that level for `MIN_STABLE`.
struct DebouncedInput<P> {
    pin: P,
    debouncer: Debouncer,
}

impl<P: InputPin> DebouncedInput<P> {
    const MIN_STABLE: Duration = Duration::from_millis(500);

    fn new(mut pin: P) -> Self {
        let initial = unwrap!(pin.is_high().ok());
        Self {
            pin,
            debouncer: Debouncer::new(initial, Self::MIN_STABLE.as_millis()),
        }
    }

    fn is_high(&mut self) -> bool {
        let level = unwrap!(self.pin.is_high().ok());
        self.debouncer.update(level, Instant::now().as_millis())
    }
}

pub struct BatteryMonitor<VBUS, CHG> {
    vbus_detect: DebouncedInput<VBUS>,
    charger_status: DebouncedInput<CHG>,
    last_battery_state: BatteryState,
    signal: TaskController<()>,
    sensor: Shared<BatterySensor>,
}

impl<VBUS, CHG> BatteryMonitor<VBUS, CHG>
where
    VBUS: InputPin,
    CHG: InputPin,
{
    pub async fn start(
        vbus_detect: VBUS,
        charger_status: CHG,
        sensor: BatterySensorDriver,
    ) -> Self {
        let this = BatteryMonitor {
            sensor: Rc::new(Mutex::new(BatterySensor {
                state: BatteryState::default(),
                sensor,
            })),
            vbus_detect: DebouncedInput::new(vbus_detect),
            charger_status: DebouncedInput::new(charger_status),
            last_battery_state: BatteryState::default(),
            signal: TaskController::new(),
        };

        let spawner = unsafe { Spawner::for_current_executor().await };
        spawner.spawn(unwrap!(monitor_task(
            this.sensor.clone(),
            this.signal.token()
        )));

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
        self.vbus_detect.is_high()
    }

    /// Reads the VBUS pin without the debounce. The deep-sleep wakeup is level-triggered, so it
    /// needs the current level: a stale one wakes the device at once.
    pub fn is_plugged_now(&mut self) -> bool {
        unwrap!(self.vbus_detect.pin.is_high().ok())
    }

    pub fn is_charging(&mut self) -> bool {
        !self.charger_status.is_high()
    }

    pub fn is_low(&mut self) -> bool {
        self.last_battery_data()
            .map(|data| data.is_low)
            .unwrap_or(false)
    }

    pub async fn sensor(&self) -> SharedGuard<'_, BatterySensor> {
        self.sensor.lock().await
    }

    pub async fn stop(self) {
        _ = self.signal.stop().await;
    }
}

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
