#![no_std]
#![feature(async_fn_in_trait)]
#![allow(incomplete_features)]

use device_descriptor::{Proxy, ReadOnlyRegister, Register};
use embedded_hal::i2c::{I2c, Operation};
use embedded_hal_async::{delay::DelayUs as AsyncDelayUs, i2c::I2c as AsyncI2c};
use register_access::{AsyncRegisterAccess, RegisterAccess};

use crate::descriptors::*;

pub mod descriptors;

#[derive(Debug)]
pub enum Error<I2cE> {
    Transfer(I2cE),
    Verify,
}

pub struct Max17055<I2C> {
    i2c: I2C,
}

impl<I2C> RegisterAccess<u16> for Max17055<I2C>
where
    I2C: I2c,
{
    type Error = Error<I2C::Error>;

    fn read_register<R>(&mut self) -> Result<R, Self::Error>
    where
        R: ReadOnlyRegister + Proxy<RegisterWidth = u16>,
    {
        let mut buffer = [0];
        self.read_sequential::<R>(&mut buffer)
            .map(|_| R::from_bits(buffer[0]))
    }

    fn read_sequential<R>(&mut self, buffer: &mut [u8]) -> Result<(), Self::Error>
    where
        R: ReadOnlyRegister + Proxy<RegisterWidth = u16>,
    {
        self.i2c
            .write_read(Self::DEVICE_ADDR, &[R::ADDRESS], buffer)
            .map_err(Error::Transfer)
    }

    fn write_register<R>(&mut self, reg: R) -> Result<(), Self::Error>
    where
        R: Register + Proxy<RegisterWidth = u16>,
    {
        self.write_sequential::<R>(&mut [reg.bits()])
    }

    fn write_sequential<R>(&mut self, buffer: &mut [u8]) -> Result<(), Self::Error>
    where
        R: Register + Proxy<RegisterWidth = u16>,
    {
        self.i2c
            .transaction(
                Self::DEVICE_ADDR,
                &mut [Operation::Write(&[R::ADDRESS]), Operation::Write(buffer)],
            )
            .map_err(Error::Transfer)
    }
}

impl<I2C> AsyncRegisterAccess<u16> for Max17055<I2C>
where
    I2C: AsyncI2c,
{
    type Error = Error<I2C::Error>;

    async fn read_register_async<R>(&mut self) -> Result<R, Self::Error>
    where
        R: ReadOnlyRegister + Proxy<RegisterWidth = u16>,
    {
        let mut buffer = [0];
        self.read_sequential_async::<R>(&mut buffer)
            .await
            .map(|_| R::from_bits(buffer[0]))
    }

    async fn read_sequential_async<R>(&mut self, buffer: &mut [u8]) -> Result<(), Self::Error>
    where
        R: ReadOnlyRegister + Proxy<RegisterWidth = u16>,
    {
        self.i2c
            .write_read(Self::DEVICE_ADDR, &[R::ADDRESS], buffer)
            .await
            .map_err(Error::Transfer)
    }

    async fn write_register_async<R>(&mut self, reg: R) -> Result<(), Self::Error>
    where
        R: Register + Proxy<RegisterWidth = u16>,
    {
        self.write_sequential_async::<R>(&mut [reg.bits()]).await
    }

    async fn write_sequential_async<R>(&mut self, buffer: &mut [u8]) -> Result<(), Self::Error>
    where
        R: Register + Proxy<RegisterWidth = u16>,
    {
        self.i2c
            .transaction(
                Self::DEVICE_ADDR,
                &mut [Operation::Write(&[R::ADDRESS]), Operation::Write(buffer)],
            )
            .await
            .map_err(Error::Transfer)
    }
}

impl<I2C> Max17055<I2C> {
    const DEVICE_ADDR: u8 = 0x36; // or << 1

    pub fn new(i2c: I2C) -> Self {
        Self { i2c }
    }
}

impl<I2C> Max17055<I2C>
where
    I2C: AsyncI2c,
{
    async fn write_and_verify_register_async<R>(
        &mut self,
        reg: R,
        delay: &mut impl AsyncDelayUs,
    ) -> Result<(), Error<I2C::Error>>
    where
        R: Register + Proxy<RegisterWidth = u16>,
    {
        for _ in 0..3 {
            self.write_sequential_async::<R>(&mut [reg.bits()]).await?;
            delay.delay_ms(1).await;
            let value = self.read_register_async::<R>().await?;
            if value.bits() == reg.bits() {
                return Ok(());
            }
        }
        Err(Error::Verify)
    }

    /// This function implements the Initialize Registers to Recommended Configuration
    /// procedure from the datasheet.
    pub async fn load_initial_config_async(
        &mut self,
        delay: &mut impl AsyncDelayUs,
    ) -> Result<(), Error<I2C::Error>> {
        let por_status = self.read_register_async::<Status>().await?;
        if por_status.por().read() == Some(PowerOnReset::Reset) {
            loop {
                let fstat = self.read_register_async::<FStat>().await?;
                if fstat.dnr().read() == Some(DataNotReady::Ready) {
                    break;
                }

                delay.wait_ms(10).await?;
            }

            let hib_cfg = self.read_register_async::<HibCfg>().await?;
            self.write_register_async(Command::new(|w| w.command().write(CommandKind::SoftWakeup)))
                .await?; // Exit Hibernate Mode step 1
            self.write_register_async::<HibCfg>(HibCfg::new(|w| {
                w.en_hib().write(Bit::NotSet).hib_config().write(0)
            }))
            .await?; // Exit Hibernate Mode step 2
            self.write_register_async(Command::new(|w| w.command().write(CommandKind::Clear)))
                .await?; // Exit Hibernate Mode step 3

            // EZ config
            // TODO: scale capacity
            const CAPACITY: u16 = 320;
            self.write_register_async(DesignCap::new(|w| w.capacity().write(CAPACITY)))
                .await?;
            self.write_register_async(dQAcc::new(|w| w.capacity().write(CAPACITY / 16)))
                .await?;
            self.write_register_async(IChgTerm::new(|w| w)).await?;
            self.write_register_async(VEmpty::new(|w| w)).await?;

            // Clear POR flag
            let status = self.read_register_async::<Status>().await?;
            self.write_and_verify_register_async(
                status.modify(|reg| reg.por().write(PowerOnReset::NoReset)),
                delay,
            )
            .await?;
        }

        // TODO: 4.3-4.10

        Ok(())
    }
}
