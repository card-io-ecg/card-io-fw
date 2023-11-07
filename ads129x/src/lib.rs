#![no_std]
#![feature(async_fn_in_trait)]
#![allow(unknown_lints, stable_features, async_fn_in_trait)]

#[macro_use]
extern crate logger;

use byteorder::{BigEndian, ByteOrder};
use device_descriptor::{ReadOnlyRegister, ReaderProxy, Register};
use embedded_hal::{
    digital::OutputPin,
    spi::{Operation, SpiDevice},
};
use embedded_hal_async::spi::SpiDevice as AsyncSpiDevice;
use register_access::{AsyncRegisterAccess, RegisterAccess};

use crate::descriptors::*;

pub mod descriptors;

// longer wait time used because esp-hal can't handle short waits yet
const WAIT_TIME_AFTER_TRANSFER: u32 = 50;
// const WAIT_TIME_AFTER_TRANSFER: u32 = 5;

#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error<SpiE> {
    InvalidState,
    UnexpectedDeviceId,
    Verification,
    Transfer(SpiE),
}

#[derive(Copy, Clone, Default, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
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

        driver.write_sequential::<Config1>(&mut config_bytes)?;
        driver.read_sequential::<Config1>(&mut config_bytes)?;

        self.verify_config(config_bytes)
    }

    pub async fn apply_async<SPI>(&self, driver: &mut Ads129x<SPI>) -> Result<(), Error<SPI::Error>>
    where
        SPI: AsyncSpiDevice,
    {
        let mut config_bytes = self.into_raw();

        driver
            .write_sequential_async::<Config1>(&mut config_bytes)
            .await?;
        driver
            .read_sequential_async::<Config1>(&mut config_bytes)
            .await?;

        self.verify_config(config_bytes)
    }

    fn verify_config<E>(&self, mut readback: [u8; 11]) -> Result<(), Error<E>> {
        let mut config_bytes = self.into_raw();

        fn mask_config(config: &mut [u8; 11]) {
            // equal chances, mask input bits

            config[7] &= 0xE0; // Lead-off status
            config[10] &= 0x0C; // GPIO data
        }

        mask_config(&mut readback);
        mask_config(&mut config_bytes);

        if config_bytes == readback {
            Ok(())
        } else {
            warn!(
                "Verification failed: received: {:?}, expected: {:?}",
                readback, config_bytes
            );
            Err(Error::Verification)
        }
    }
}

pub struct Ads129x<SPI> {
    spi: SPI,
}

impl<SPI> RegisterAccess<u8> for Ads129x<SPI>
where
    SPI: SpiDevice,
{
    type Error = Error<SPI::Error>;

    fn read_register<R>(&mut self) -> Result<R, Self::Error>
    where
        R: ReadOnlyRegister<RegisterWidth = u8>,
    {
        let mut buffer = [0];
        self.read_sequential::<R>(&mut buffer)
            .map(|_| R::from_bits(buffer[0]))
    }

    fn read_sequential<R>(&mut self, buffer: &mut [u8]) -> Result<(), Self::Error>
    where
        R: ReadOnlyRegister<RegisterWidth = u8>,
    {
        self.write_command_internal(Self::start_read_command::<R>(buffer), buffer)
    }

    fn write_register<R>(&mut self, reg: R) -> Result<(), Self::Error>
    where
        R: Register<RegisterWidth = u8>,
    {
        self.write_sequential::<R>(&mut [reg.bits()])
    }

    fn write_sequential<R>(&mut self, buffer: &mut [u8]) -> Result<(), Self::Error>
    where
        R: Register<RegisterWidth = u8>,
    {
        self.write_command_internal(Self::start_write_command::<R>(buffer), buffer)
    }
}

impl<SPI> AsyncRegisterAccess<u8> for Ads129x<SPI>
where
    SPI: AsyncSpiDevice,
{
    type Error = Error<SPI::Error>;

    async fn read_register_async<R>(&mut self) -> Result<R, Self::Error>
    where
        R: ReadOnlyRegister<RegisterWidth = u8>,
    {
        let mut buffer = [0];
        self.read_sequential_async::<R>(&mut buffer)
            .await
            .map(|_| R::from_bits(buffer[0]))
    }

    async fn read_sequential_async<R>(&mut self, buffer: &mut [u8]) -> Result<(), Self::Error>
    where
        R: ReadOnlyRegister<RegisterWidth = u8>,
    {
        self.write_command_internal_async(Self::start_read_command::<R>(buffer), buffer)
            .await
    }

    async fn write_register_async<R>(&mut self, reg: R) -> Result<(), Self::Error>
    where
        R: Register<RegisterWidth = u8>,
    {
        self.write_sequential_async::<R>(&mut [reg.bits()]).await
    }

    async fn write_sequential_async<R>(&mut self, buffer: &mut [u8]) -> Result<(), Self::Error>
    where
        R: Register<RegisterWidth = u8>,
    {
        self.write_command_internal_async(Self::start_write_command::<R>(buffer), buffer)
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

    fn start_write_command<R: Register>(buf: &[u8]) -> Command {
        Command::WREG(R::ADDRESS, buf.len() as u8)
    }

    fn start_read_command<R: ReadOnlyRegister>(buf: &[u8]) -> Command {
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
    pub fn read_data_1ch(&mut self) -> Result<AdsData, Error<SPI::Error>> {
        let mut sample: [u8; 6] = [0; 6];
        self.spi
            .read(&mut sample)
            .map(|_| AdsData::new_single_channel(sample))
            .map_err(Error::Transfer)
    }

    pub fn read_data_2ch(&mut self) -> Result<AdsData, Error<SPI::Error>> {
        let mut sample: [u8; 9] = [0; 9];
        self.spi
            .read(&mut sample)
            .map(|_| AdsData::new(sample))
            .map_err(Error::Transfer)
    }

    fn write_command_internal(
        &mut self,
        command: Command,
        payload: &mut [u8],
    ) -> Result<(), Error<SPI::Error>> {
        let (bytes, len) = command.into();
        let command = &bytes[0..len];

        if payload.is_empty() {
            self.spi
                .transaction(&mut [
                    Operation::Write(command),
                    Operation::DelayUs(WAIT_TIME_AFTER_TRANSFER),
                ])
                .map_err(Error::Transfer)
        } else {
            self.spi
                .transaction(&mut [
                    Operation::Write(command),
                    Operation::TransferInPlace(payload),
                    Operation::DelayUs(WAIT_TIME_AFTER_TRANSFER),
                ])
                .map_err(Error::Transfer)
        }
    }

    pub fn write_command(&mut self, command: Command) -> Result<(), Error<SPI::Error>> {
        self.write_command_internal(command, &mut [])
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
        unwrap!(reset.set_high().ok());
        delay.delay_ms(Self::MIN_T_POR);
        unwrap!(reset.set_low().ok());
        delay.delay_ms(Self::MIN_T_RST);
        unwrap!(reset.set_high().ok());
        delay.delay_ms(Self::MIN_RST_WAIT);
    }
}

impl<SPI> Ads129x<SPI>
where
    SPI: AsyncSpiDevice,
{
    pub async fn read_data_1ch_async_rdatac(&mut self) -> Result<AdsData, Error<SPI::Error>> {
        let mut sample: [u8; 6] = [0; 6];
        self.spi
            .read(&mut sample)
            .await
            .map(|_| AdsData::new_single_channel(sample))
            .map_err(Error::Transfer)
    }

    pub async fn read_data_1ch_async(&mut self) -> Result<AdsData, Error<SPI::Error>> {
        let mut buffer: [u8; 8] = [0; 8];
        let (command, bytes) = <([u8; 2], usize)>::from(Command::RDATA);
        buffer[0..bytes].copy_from_slice(&command[0..bytes]);

        self.spi
            .transaction(&mut [Operation::TransferInPlace(&mut buffer)])
            .await
            .map_err(Error::Transfer)?;

        Ok(AdsData::new_single_channel(unwrap!(buffer
            [bytes..bytes + 6]
            .try_into())))
    }

    pub async fn read_data_2ch_async_rdatac(&mut self) -> Result<AdsData, Error<SPI::Error>> {
        let mut sample: [u8; 9] = [0; 9];
        self.spi
            .read(&mut sample)
            .await
            .map(|_| AdsData::new(sample))
            .map_err(Error::Transfer)
    }

    pub async fn read_data_2ch_async(&mut self) -> Result<AdsData, Error<SPI::Error>> {
        let mut buffer: [u8; 11] = [0; 11];
        let (command, bytes) = <([u8; 2], usize)>::from(Command::RDATA);
        buffer[0..bytes].copy_from_slice(&command[0..bytes]);

        self.spi
            .transaction(&mut [Operation::TransferInPlace(&mut buffer)])
            .await
            .map_err(Error::Transfer)?;

        Ok(AdsData::new(unwrap!(buffer[bytes..bytes + 9].try_into())))
    }

    async fn write_command_internal_async(
        &mut self,
        command: Command,
        payload: &mut [u8],
    ) -> Result<(), Error<SPI::Error>> {
        let (bytes, len) = command.into();
        let command = &bytes[0..len];

        if payload.is_empty() {
            self.spi
                .transaction(&mut [
                    Operation::Write(command),
                    Operation::DelayUs(WAIT_TIME_AFTER_TRANSFER),
                ])
                .await
                .map_err(Error::Transfer)
        } else {
            self.spi
                .transaction(&mut [
                    Operation::Write(command),
                    Operation::TransferInPlace(payload),
                    Operation::DelayUs(WAIT_TIME_AFTER_TRANSFER),
                ])
                .await
                .map_err(Error::Transfer)
        }
    }

    pub async fn write_command_async(&mut self, command: Command) -> Result<(), Error<SPI::Error>> {
        self.write_command_internal_async(command, &mut []).await
    }

    pub async fn read_device_id_async(&mut self) -> Result<DeviceId, Error<SPI::Error>> {
        let read_result = self.read_register_async::<Id>().await?.id();
        match read_result.read() {
            Some(id) => Ok(id),
            None => {
                warn!("Read unknown device id: {}", read_result.read_field_bits());
                Err(Error::UnexpectedDeviceId)
            }
        }
    }

    pub async fn apply_configuration_async(
        &mut self,
        config: &ConfigRegisters,
    ) -> Result<(), Error<SPI::Error>> {
        config.apply_async(self).await
    }

    pub async fn reset_async<RESET>(
        &mut self,
        reset: &mut RESET,
        delay: &mut impl embedded_hal_async::delay::DelayUs,
    ) -> Result<(), Error<SPI::Error>>
    where
        RESET: OutputPin,
    {
        unwrap!(reset.set_high().ok());
        delay.delay_ms(Self::MIN_T_POR).await;
        unwrap!(reset.set_low().ok());
        delay.delay_ms(Self::MIN_T_RST).await;
        unwrap!(reset.set_high().ok());
        delay.delay_ms(Self::MIN_RST_WAIT).await;

        self.write_command_async(Command::SDATAC).await
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Sample {
    sample: i32,
}

impl Sample {
    pub const VOLTS_PER_LSB: f32 = 2.42 / (1 << 23) as f32;

    #[inline]
    pub fn voltage(self) -> f32 {
        (self.sample as f32) * Self::VOLTS_PER_LSB
    }

    #[inline]
    pub fn raw(self) -> i32 {
        self.sample
    }
}

#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct AdsData {
    status: LoffStat,
    ch1: Sample,
    ch2: Sample,
}

impl AdsData {
    fn read_status(buffer: [u8; 3]) -> LoffStat {
        LoffStat::from_bits((buffer[0] << 1 | buffer[1] >> 7) & 0x1F)
    }

    fn read_channel(buffer: [u8; 3]) -> Sample {
        Sample {
            sample: BigEndian::read_i24(&buffer),
        }
    }

    #[inline]
    pub fn new(buffer: [u8; 9]) -> Self {
        Self {
            status: Self::read_status(unwrap!(buffer[0..3].try_into())),
            ch1: Self::read_channel(unwrap!(buffer[3..6].try_into())),
            ch2: Self::read_channel(unwrap!(buffer[6..9].try_into())),
        }
    }

    #[inline]
    pub fn new_single_channel(buffer: [u8; 6]) -> Self {
        Self {
            status: Self::read_status(unwrap!(buffer[0..3].try_into())),
            ch1: Self::read_channel(unwrap!(buffer[3..6].try_into())),
            ch2: Sample { sample: 0 },
        }
    }

    #[inline]
    pub fn ch1_positive_lead_connected(&self) -> bool {
        self.status.in1p().read() == Some(LeadStatus::Connected)
    }

    #[inline]
    pub fn ch1_negative_lead_connected(&self) -> bool {
        self.status.in1n().read() == Some(LeadStatus::Connected)
    }

    #[inline]
    pub fn ch1_leads_connected(&self) -> bool {
        self.ch1_negative_lead_connected() && self.ch1_positive_lead_connected()
    }

    #[inline]
    pub fn ch2_positive_lead_connected(&self) -> bool {
        self.status.in2p().read() == Some(LeadStatus::Connected)
    }

    #[inline]
    pub fn ch2_negative_lead_connected(&self) -> bool {
        self.status.in2n().read() == Some(LeadStatus::Connected)
    }

    #[inline]
    pub fn ch2_leads_connected(&self) -> bool {
        self.ch2_negative_lead_connected() && self.ch2_positive_lead_connected()
    }

    #[inline]
    pub fn ch1_sample(&self) -> Sample {
        self.ch1
    }

    #[inline]
    pub fn ch2_sample(&self) -> Sample {
        self.ch2
    }
}
