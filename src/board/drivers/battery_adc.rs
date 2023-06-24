use crate::board::hal::{
    adc::{AdcConfig, AdcPin, Attenuation, RegisterAccess, ADC},
    prelude::*,
};
use embassy_futures::yield_now;
use embedded_hal_old::adc::{Channel, OneShot};
use esp32s3_hal::{efuse::Efuse, peripheral::Peripheral};

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
