#![no_std]
#![feature(async_fn_in_trait)]
#![allow(incomplete_features)]

use byteorder::{BigEndian, ByteOrder};
use device_descriptor::{Proxy, ReadOnlyRegister, Register};
use embedded_hal::{
    digital::OutputPin,
    spi::{Operation, SpiDevice},
};
use embedded_hal_async::spi::SpiDevice as AsyncSpiDevice;
use register_access::{AsyncRegisterAccess, RegisterAccess};

use crate::descriptors::*;

pub mod descriptors;

#[derive(Debug)]
pub enum Error<SpiE> {
    InvalidState,
    UnexpectedDeviceId,
    Verification,
    Transfer(SpiE),
}

#[derive(Copy, Clone, Debug, Default)]
pub struct ConfigRegisters {
    pub config1: Config1,
    pub config2: Config2,
    pub loff: Loff,
    pub ch1set: Ch1Set,
    pub ch2set: Ch2Set,
    pub rldsens: RldSens,
    pub loffsens: LoffSens,
    pub loffstat: LoffStat,
    pub resp1: Resp1,
    pub resp2: Resp2,
    pub gpio: Gpio,
}

impl ConfigRegisters {
    fn into_raw(self) -> [u8; 11] {
        [
            self.config1.bits(),
            self.config2.bits(),
            self.loff.bits(),
            self.ch1set.bits(),
            self.ch2set.bits(),
            self.rldsens.bits(),
            self.loffsens.bits(),
            self.loffstat.bits(),
            self.resp1.bits(),
            self.resp2.bits(),
            self.gpio.bits(),
        ]
    }

    pub fn apply<SPI>(&self, driver: &mut Ads129x<SPI>) -> Result<(), Error<SPI::Error>>
    where
        SPI: SpiDevice,
    {
        let mut config_bytes = self.into_raw();
        let mut readback = [0; 11];

        driver.write_sequential::<Config1>(&mut config_bytes)?;
        driver.read_sequential::<Config1>(&mut readback)?;

        if Self::verify_readback(&mut config_bytes, &mut readback) {
            Ok(())
        } else {
            Err(Error::Verification)
        }
    }

    pub async fn apply_async<SPI>(&self, driver: &mut Ads129x<SPI>) -> Result<(), Error<SPI::Error>>
    where
        SPI: AsyncSpiDevice,
    {
        let mut config_bytes = self.into_raw();
        let mut readback = [0; 11];

        driver
            .write_sequential_async::<Config1>(&mut config_bytes)
            .await?;
        driver
            .read_sequential_async::<Config1>(&mut readback)
            .await?;

        if Self::verify_readback(&mut config_bytes, &mut readback) {
            Ok(())
        } else {
            Err(Error::Verification)
        }
    }

    fn verify_readback(config_bytes: &mut [u8; 11], readback: &mut [u8; 11]) -> bool {
        // equal chances, mask input bits
        config_bytes[7] &= 0xE0;
        config_bytes[10] &= 0x0C;

        readback[7] &= 0xE0;
        readback[10] &= 0x0C;

        config_bytes == readback
    }
}

pub struct Ads129x<SPI> {
    spi: SPI,
}

impl<SPI> RegisterAccess for Ads129x<SPI>
where
    SPI: SpiDevice,
{
    type Error = Error<SPI::Error>;

    fn read_register<R: ReadOnlyRegister<u8>>(&mut self) -> Result<R, Self::Error> {
        let mut buffer = [0];
        self.read_sequential::<R>(&mut buffer)
            .map(|_| R::from_bits(buffer[0]))
    }

    fn read_sequential<R: ReadOnlyRegister<u8>>(
        &mut self,
        buffer: &mut [u8],
    ) -> Result<(), Self::Error> {
        self.write_command(Self::start_read_command::<R>(buffer), buffer)
    }

    fn write_register<R: Register<u8>>(&mut self, reg: R) -> Result<(), Self::Error> {
        self.write_sequential::<R>(&mut [reg.bits()])
    }

    fn write_sequential<R: Register<u8>>(&mut self, buffer: &mut [u8]) -> Result<(), Self::Error> {
        self.write_command(Self::start_write_command::<R>(buffer), buffer)
    }
}

impl<SPI> AsyncRegisterAccess for Ads129x<SPI>
where
    SPI: AsyncSpiDevice,
{
    type Error = Error<SPI::Error>;

    async fn read_register_async<R: ReadOnlyRegister<u8>>(&mut self) -> Result<R, Self::Error> {
        let mut buffer = [0];
        self.read_sequential_async::<R>(&mut buffer)
            .await
            .map(|_| R::from_bits(buffer[0]))
    }

    async fn read_sequential_async<R: ReadOnlyRegister<u8>>(
        &mut self,
        buffer: &mut [u8],
    ) -> Result<(), Self::Error> {
        self.write_command_async(Self::start_read_command::<R>(buffer), buffer)
            .await
    }

    async fn write_register_async<R: Register<u8>>(&mut self, reg: R) -> Result<(), Self::Error> {
        self.write_sequential_async::<R>(&mut [reg.bits()]).await
    }

    async fn write_sequential_async<R: Register<u8>>(
        &mut self,
        buffer: &mut [u8],
    ) -> Result<(), Self::Error> {
        self.write_command_async(Self::start_write_command::<R>(buffer), buffer)
            .await
    }
}

impl<SPI> Ads129x<SPI> {
    // t_mod = 1/128kHz
    const MIN_T_POR: u32 = 32; // >= 4096 * t_mod >= 1/32s
    const MIN_T_RST: u32 = 1; // >= 1 * t_mod >= 8us
    const MIN_RST_WAIT: u32 = 1; // >= 18 * t_mod >= 140us

    pub const fn new(spi: SPI) -> Self {
        Self { spi }
    }

    fn start_write_command<R: Register<u8>>(buf: &[u8]) -> Command {
        Command::WREG(R::ADDRESS, buf.len() as u8)
    }

    fn start_read_command<R: ReadOnlyRegister<u8>>(buf: &[u8]) -> Command {
        Command::RREG(R::ADDRESS, buf.len() as u8)
    }

    pub fn inner_mut(&mut self) -> &mut SPI {
        &mut self.spi
    }

    pub fn into_inner(self) -> SPI {
        self.spi
    }
}

impl<SPI> Ads129x<SPI>
where
    SPI: SpiDevice,
{
    pub fn read_data_1ch(&mut self) -> Result<Sample, Error<SPI::Error>> {
        let mut sample: [u8; 6] = [0; 6];
        self.spi
            .read(&mut sample)
            .map(|_| Sample::new_single_channel(sample))
            .map_err(Error::Transfer)
    }

    pub fn read_data_2ch(&mut self) -> Result<Sample, Error<SPI::Error>> {
        let mut sample: [u8; 9] = [0; 9];
        self.spi
            .read(&mut sample)
            .map(|_| Sample::new(sample))
            .map_err(Error::Transfer)
    }

    pub fn write_command(
        &mut self,
        command: Command,
        payload: &mut [u8],
    ) -> Result<(), Error<SPI::Error>> {
        let (bytes, len) = command.into();

        self.spi
            .transaction(&mut [
                Operation::Write(&bytes[0..len]),
                Operation::TransferInPlace(payload),
            ])
            .map_err(Error::Transfer)
    }

    pub fn apply_configuration(
        &mut self,
        config: &ConfigRegisters,
    ) -> Result<(), Error<SPI::Error>> {
        config.apply(self)
    }

    pub fn reset<RESET>(&self, reset: &mut RESET, delay: &mut impl embedded_hal::delay::DelayUs)
    where
        RESET: OutputPin,
    {
        reset.set_high().unwrap();
        delay.delay_ms(Self::MIN_T_POR);
        reset.set_low().unwrap();
        delay.delay_ms(Self::MIN_T_RST);
        reset.set_high().unwrap();
        delay.delay_ms(Self::MIN_RST_WAIT);
    }
}

impl<SPI> Ads129x<SPI>
where
    SPI: AsyncSpiDevice,
{
    pub async fn read_data_1ch_async(&mut self) -> Result<Sample, Error<SPI::Error>> {
        let mut sample: [u8; 6] = [0; 6];
        self.spi
            .read(&mut sample)
            .await
            .map(|_| Sample::new_single_channel(sample))
            .map_err(Error::Transfer)
    }

    pub async fn read_data_2ch_async(&mut self) -> Result<Sample, Error<SPI::Error>> {
        let mut sample: [u8; 9] = [0; 9];
        self.spi
            .read(&mut sample)
            .await
            .map(|_| Sample::new(sample))
            .map_err(Error::Transfer)
    }

    pub async fn write_command_async(
        &mut self,
        command: Command,
        payload: &mut [u8],
    ) -> Result<(), Error<SPI::Error>> {
        let (bytes, len) = command.into();

        self.spi
            .transaction(&mut [
                Operation::Write(&bytes[0..len]),
                Operation::TransferInPlace(payload),
            ])
            .await
            .map_err(Error::Transfer)
    }

    pub async fn apply_configuration_async(
        &mut self,
        config: &ConfigRegisters,
    ) -> Result<(), Error<SPI::Error>> {
        config.apply_async(self).await
    }

    pub async fn reset_async<RESET>(
        &self,
        reset: &mut RESET,
        delay: &mut impl embedded_hal_async::delay::DelayUs,
    ) where
        RESET: OutputPin,
    {
        reset.set_high().unwrap();
        delay.delay_ms(Self::MIN_T_POR).await;
        reset.set_low().unwrap();
        delay.delay_ms(Self::MIN_T_RST).await;
        reset.set_high().unwrap();
        delay.delay_ms(Self::MIN_RST_WAIT).await;
    }
}

pub struct Sample {
    status: LoffStat,
    ch1: i32,
    ch2: i32,
}

impl Sample {
    pub const VOLTS_PER_LSB: f32 = 2.42 / (1 << 23) as f32;

    fn read_status(buffer: [u8; 3]) -> LoffStat {
        LoffStat::from_bits((buffer[0] << 1 | buffer[1] >> 7) & 0x1F)
    }

    fn read_channel(buffer: [u8; 3]) -> i32 {
        BigEndian::read_i24(&buffer)
    }

    fn new(buffer: [u8; 9]) -> Self {
        Self {
            status: Self::read_status(buffer[0..3].try_into().unwrap()),
            ch1: Self::read_channel(buffer[3..6].try_into().unwrap()),
            ch2: Self::read_channel(buffer[6..9].try_into().unwrap()),
        }
    }

    fn new_single_channel(buffer: [u8; 6]) -> Self {
        Self {
            status: Self::read_status(buffer[0..3].try_into().unwrap()),
            ch1: Self::read_channel(buffer[3..6].try_into().unwrap()),
            ch2: 0,
        }
    }

    pub fn ch1_leads_connected(&self) -> bool {
        self.status.in1n().read() == Some(LeadStatus::Connected)
            && self.status.in1p().read() == Some(LeadStatus::Connected)
    }

    pub fn ch2_leads_connected(&self) -> bool {
        self.status.in2n().read() == Some(LeadStatus::Connected)
            && self.status.in2p().read() == Some(LeadStatus::Connected)
    }

    pub fn ch1_sample(&self) -> i32 {
        self.ch1
    }

    pub fn ch2_sample(&self) -> i32 {
        self.ch2
    }

    pub fn ch1_voltage(&self) -> f32 {
        (self.ch1 as f32) * Self::VOLTS_PER_LSB
    }

    pub fn ch2_voltage(&self) -> f32 {
        (self.ch2 as f32) * Self::VOLTS_PER_LSB
    }
}
