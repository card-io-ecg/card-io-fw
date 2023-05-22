use crate::board::hal::{
    adc::{AdcConfig, AdcPin, Attenuation, RegisterAccess, ADC},
    prelude::*,
};
use embassy_futures::yield_now;
use embedded_hal_old::adc::{Channel, OneShot};
use esp32s3_hal::peripheral::Peripheral;

pub struct BatteryAdc<V, A, EN, ADCI: 'static> {
    pub voltage_in: AdcPin<V, ADCI>,
    pub current_in: AdcPin<A, ADCI>,
    pub enable: EN,
    pub adc: ADC<'static, ADCI>,
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
        }
    }

    fn raw_to_mv(&self, raw: u16) -> u16 {
        raw
    }

    pub async fn read_battery_voltage(&mut self) -> Result<u16, ()> {
        loop {
            match self.adc.read(&mut self.voltage_in) {
                Ok(out) => return Ok(self.raw_to_mv(out * 2)), // 2x Voltage divider
                Err(nb::Error::Other(_e)) => return Err(()),
                Err(nb::Error::WouldBlock) => yield_now().await,
            }
        }
    }

    pub async fn read_charge_current(&mut self) -> Result<u16, ()> {
        loop {
            match self.adc.read(&mut self.current_in) {
                Ok(out) => return Ok(self.raw_to_mv(out)),
                Err(nb::Error::Other(_e)) => return Err(()),
                Err(nb::Error::WouldBlock) => yield_now().await,
            }
        }
    }
}
