use crate::{
    board::hal::{
        adc::{AdcConfig, AdcPin, Attenuation, RegisterAccess, ADC},
        efuse::Efuse,
        peripheral::Peripheral,
        prelude::*,
    },
    SharedBatteryState,
};
use embassy_futures::yield_now;
use embassy_sync::{blocking_mutex::raw::NoopRawMutex, signal::Signal};
use embassy_time::{Duration, Ticker};
use embedded_hal_old::adc::{Channel, OneShot};

#[derive(Clone, Copy, Debug)]
pub struct BatteryAdcData {
    pub voltage: u16,
    pub charge_current: u16,
}

struct AdcCalibration {
    // Assumption is that this is the ADC output @ 850mV
    calibration_factor: u32,
}

impl AdcCalibration {
    fn new(attenuation: Attenuation) -> Self {
        let (d, _v) = Efuse::adc2_get_cal_voltage(attenuation);

        Self {
            calibration_factor: 100_000 * 925 / d,
        }
    }

    fn raw_to_mv(&self, raw: u16) -> u16 {
        ((raw as u32 * self.calibration_factor) / 100_000) as u16
    }
}

pub struct BatteryAdc<V, A, EN, ADCI: 'static> {
    pub voltage_in: AdcPin<V, ADCI>,
    pub current_in: AdcPin<A, ADCI>,
    pub enable: EN,
    pub adc: ADC<'static, ADCI>,
    calibration: AdcCalibration,
}

impl<V, A, EN, ADCI> BatteryAdc<V, A, EN, ADCI>
where
    ADCI: RegisterAccess + 'static + Peripheral<P = ADCI>,
    ADC<'static, ADCI>: OneShot<ADCI, u16, AdcPin<V, ADCI>>,
    ADC<'static, ADCI>: OneShot<ADCI, u16, AdcPin<A, ADCI>>,
    V: Channel<ADCI, ID = u8>,
    A: Channel<ADCI, ID = u8>,
{
    pub fn new(adc: ADCI, voltage_in: V, current_in: A, enable: EN) -> Self {
        let mut adc_config = AdcConfig::new();

        Self {
            voltage_in: adc_config.enable_pin(voltage_in, Attenuation::Attenuation11dB),
            current_in: adc_config.enable_pin(current_in, Attenuation::Attenuation11dB),
            enable,
            adc: ADC::adc(adc, adc_config).unwrap(),
            calibration: AdcCalibration::new(Attenuation::Attenuation11dB),
        }
    }

    pub async fn read_battery_voltage(&mut self) -> Result<u16, ()> {
        loop {
            match self.adc.read(&mut self.voltage_in) {
                Ok(out) => {
                    return Ok(self
                        .calibration
                        .raw_to_mv(((out as u32 * 4200) / 2000) as u16)); // 2x Voltage divider + some weirdness
                }
                Err(nb::Error::Other(_e)) => return Err(()),
                Err(nb::Error::WouldBlock) => yield_now().await,
            }
        }
    }

    pub async fn read_charge_current(&mut self) -> Result<u16, ()> {
        loop {
            match self.adc.read(&mut self.current_in) {
                Ok(out) => return Ok(self.calibration.raw_to_mv(out)),
                Err(nb::Error::Other(_e)) => return Err(()),
                Err(nb::Error::WouldBlock) => yield_now().await,
            }
        }
    }

    pub async fn read_data(&mut self) -> Result<BatteryAdcData, ()> {
        Ok(BatteryAdcData {
            voltage: self.read_battery_voltage().await?,
            charge_current: self.read_charge_current().await?,
        })
    }
}

#[embassy_executor::task]
pub async fn monitor_task_adc(
    mut battery: crate::board::BatteryAdc,
    battery_state: &'static SharedBatteryState,
    task_control: &'static Signal<NoopRawMutex, ()>,
) {
    let mut timer = Ticker::every(Duration::from_millis(10));
    log::info!("ADC monitor started");

    battery.enable.set_high().unwrap();

    let mut voltage_accumulator = 0;
    let mut current_accumulator = 0;

    let mut sample_count = 0;

    const AVG_SAMPLE_COUNT: u32 = 128;

    while !task_control.signaled() {
        let data = battery.read_data().await.unwrap();

        voltage_accumulator += data.voltage as u32;
        current_accumulator += data.charge_current as u32;

        if sample_count == AVG_SAMPLE_COUNT {
            let mut state = battery_state.lock().await;

            let average = BatteryAdcData {
                voltage: (voltage_accumulator / AVG_SAMPLE_COUNT) as u16,
                charge_current: (current_accumulator / AVG_SAMPLE_COUNT) as u16,
            };
            state.adc_data = Some(average);

            log::debug!("Battery data: {average:?}");

            sample_count = 0;

            voltage_accumulator = 0;
            current_accumulator = 0;
        } else {
            sample_count += 1;
        }

        timer.next().await;
    }

    battery.enable.set_low().unwrap();

    log::info!("Monitor exited");
}
