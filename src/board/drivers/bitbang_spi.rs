use core::convert::Infallible;

use crate::hal::clock::Clocks;
use embassy_time::{Duration, Ticker};
use embedded_hal::{
    digital::{InputPin, OutputPin, PinState},
    spi::ErrorType,
};
use embedded_hal_async::spi::SpiBus;
use fugit::HertzU32;

pub struct BitbangSpi<MOSI, MISO, SCLK> {
    mosi: MOSI,
    miso: MISO,
    sclk: SCLK,
    half_bit_delay: Duration,
}

impl<MOSI, MISO, SCLK> BitbangSpi<MOSI, MISO, SCLK>
where
    MOSI: OutputPin,
    MISO: InputPin,
    SCLK: OutputPin,
{
    pub const fn new(mosi: MOSI, miso: MISO, sclk: SCLK, frequency: HertzU32) -> Self {
        Self {
            mosi,
            miso,
            sclk,
            half_bit_delay: Self::frequency_to_duration(frequency),
        }
    }

    const fn frequency_to_duration(frequency: HertzU32) -> Duration {
        let bit_duration: fugit::Duration<u32, 1, { embassy_time::TICK_HZ as u32 }> =
            frequency.into_duration();
        let clock_ticks = bit_duration.ticks() / 2;
        Duration::from_ticks(clock_ticks as u64)
    }

    pub async fn transfer_byte(&mut self, write: u8, out: &mut u8) {
        let mut ticker = Ticker::every(self.half_bit_delay);
        *out = 0;
        for i in (0..8).rev() {
            ticker.next().await;
            self.sclk.set_high().unwrap();
            self.mosi
                .set_state(PinState::from(write & (1 << i) != 0))
                .unwrap();

            ticker.next().await;
            self.sclk.set_low().unwrap();
            *out |= (self.miso.is_high().unwrap() as u8) << i;
        }
    }

    pub fn change_bus_frequency(&mut self, frequency: HertzU32, _clocks: &Clocks<'_>) {
        self.half_bit_delay = Self::frequency_to_duration(frequency);
    }
}

impl<MOSI, MISO, SCLK> ErrorType for BitbangSpi<MOSI, MISO, SCLK>
where
    MOSI: OutputPin,
    MISO: InputPin,
    SCLK: OutputPin,
{
    type Error = Infallible;
}

impl<MOSI, MISO, SCLK> SpiBus for BitbangSpi<MOSI, MISO, SCLK>
where
    MOSI: OutputPin,
    MISO: InputPin,
    SCLK: OutputPin,
{
    async fn read(&mut self, bytes: &mut [u8]) -> Result<(), Self::Error> {
        for byte in bytes {
            self.transfer_byte(0, byte).await;
        }
        Ok(())
    }

    async fn write(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        for byte in bytes {
            self.transfer_byte(*byte, &mut 0).await;
        }
        Ok(())
    }

    async fn transfer(&mut self, read: &mut [u8], write: &[u8]) -> Result<(), Self::Error> {
        for (read, write) in read.iter_mut().zip(write.iter()) {
            self.transfer_byte(*write, read).await;
        }
        Ok(())
    }

    async fn transfer_in_place(&mut self, bytes: &mut [u8]) -> Result<(), Self::Error> {
        for byte in bytes {
            self.transfer_byte(*byte, byte).await;
        }
        Ok(())
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}
