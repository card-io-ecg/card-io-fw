use crate::SharedBatteryState;
use embassy_sync::{blocking_mutex::raw::NoopRawMutex, mutex::Mutex, signal::Signal};
use embedded_hal::digital::InputPin;
use gui::screens::BatteryInfo;

#[cfg(feature = "battery_adc")]
use crate::board::{drivers::battery_adc, drivers::battery_adc::BatteryAdcData, BatteryAdc};

#[cfg(feature = "battery_adc")]
use signal_processing::battery::BatteryModel;

#[cfg(feature = "battery_max17055")]
use crate::board::{drivers::battery_fg, drivers::battery_fg::BatteryFgData, BatteryFg};

#[cfg(any(feature = "battery_adc", feature = "battery_max17055"))]
use crate::board::LOW_BATTERY_PERCENTAGE;

#[cfg(any(feature = "battery_adc", feature = "battery_max17055"))]
use embassy_executor::Spawner;

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
    pub fn is_plugged(&self) -> bool {
        self.vbus_detect.is_high().unwrap()
    }

    pub fn is_charging(&self) -> bool {
        self.charger_status.is_low().unwrap()
    }

    pub async fn stop(&mut self) {
        self.signal.signal(());
    }

    #[cfg(not(any(feature = "battery_max17055", feature = "battery_adc")))]
    pub fn battery_data(&mut self) -> Option<BatteryInfo> {
        None
    }
}

#[cfg(feature = "battery_adc")]
impl<VBUS: InputPin, CHG: InputPin> BatteryMonitor<VBUS, CHG> {
    pub async fn start(
        &mut self,
        adc: BatteryAdc,
        battery_state: &'static Mutex<NoopRawMutex, BatteryState>,
    ) {
        let spawner = Spawner::for_current_executor().await;
        spawner
            .spawn(battery_adc::monitor_task_adc(
                adc,
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
}

#[cfg(feature = "battery_max17055")]
impl<VBUS: InputPin, CHG: InputPin> BatteryMonitor<VBUS, CHG> {
    pub async fn start(
        &mut self,
        fg: BatteryFg,
        battery_state: &'static Mutex<NoopRawMutex, BatteryState>,
    ) {
        let spawner = Spawner::for_current_executor().await;
        spawner
            .spawn(battery_fg::monitor_task_fg(fg, battery_state, self.signal))
            .ok();
    }

    fn load_battery_data(&mut self) {
        if let Ok(state) = self.battery_state.try_lock() {
            self.last_battery_state = *state;
        }
    }

    pub fn battery_data(&mut self) -> Option<BatteryInfo> {
        self.load_battery_data();

        self.last_battery_state.fg_data.map(|state| BatteryInfo {
            voltage: state.voltage,
            is_charging: self.is_charging(),
            percentage: state.percentage,
            is_low: state.percentage < LOW_BATTERY_PERCENTAGE,
        })
    }
}
