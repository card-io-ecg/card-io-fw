use crate::board::hal::{
    adc::{AdcPin, RegisterAccess, ADC},
    prelude::*,
};
use embassy_futures::yield_now;
use embedded_hal_old::adc::{Channel, OneShot};

pub struct BatteryAdc<V, A, EN, ADCI: 'static> {
    pub voltage_in: AdcPin<V, ADCI>,
    pub current_in: AdcPin<A, ADCI>,
    pub enable: EN,
    pub adc: ADC<'static, ADCI>,
}

impl<V, A, EN, ADCI> BatteryAdc<V, A, EN, ADCI>
where
    ADCI: RegisterAccess + 'static,
{
    pub async fn read_battery_voltage(&mut self) -> Result<u16, ()>
    where
        V: Channel<ADCI, ID = u8>,
    {
        loop {
            match self.adc.read(&mut self.voltage_in) {
                Ok(out) => return Ok(out),
                Err(nb::Error::Other(_e)) => return Err(()),
                Err(nb::Error::WouldBlock) => yield_now().await,
            }
        }
    }

    pub async fn read_charge_current(&mut self) -> Result<u16, ()>
    where
        A: Channel<ADCI, ID = u8>,
    {
        loop {
            match self.adc.read(&mut self.current_in) {
                Ok(out) => return Ok(out),
                Err(nb::Error::Other(_e)) => return Err(()),
                Err(nb::Error::WouldBlock) => yield_now().await,
            }
        }
    }
}
