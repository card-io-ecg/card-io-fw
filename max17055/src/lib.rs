#![no_std]
#![feature(async_fn_in_trait)]
#![allow(incomplete_features)]

use device_descriptor::{ReadOnlyRegister, Register};
use embedded_hal::i2c::{I2c, Operation};
use embedded_hal_async::i2c::I2c as AsyncI2c;
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

impl<I2C> RegisterAccess for Max17055<I2C>
where
    I2C: I2c,
{
    type Error = Error<I2C::Error>;

    fn read_register<R: ReadOnlyRegister<u8>>(&mut self) -> Result<R, Self::Error> {
        let mut buffer = [0];
        self.read_sequential::<R>(&mut buffer)
            .map(|_| R::from_bits(buffer[0]))
    }

    fn read_sequential<R: ReadOnlyRegister<u8>>(
        &mut self,
        buffer: &mut [u8],
    ) -> Result<(), Self::Error> {
        self.i2c
            .write_read(Self::DEVICE_ADDR, &[R::ADDRESS], buffer)
            .map_err(Error::Transfer)
    }

    fn write_register<R: Register<u8>>(&mut self, reg: R) -> Result<(), Self::Error> {
        self.write_sequential::<R>(&mut [reg.bits()])
    }

    fn write_sequential<R: Register<u8>>(&mut self, buffer: &mut [u8]) -> Result<(), Self::Error> {
        self.i2c
            .transaction(
                Self::DEVICE_ADDR,
                &mut [Operation::Write(&[R::ADDRESS]), Operation::Write(buffer)],
            )
            .map_err(Error::Transfer)
    }
}

impl<I2C> AsyncRegisterAccess for Max17055<I2C>
where
    I2C: AsyncI2c,
{
    type Error = Error<I2C::Error>;

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
        self.i2c
            .write_read(Self::DEVICE_ADDR, &[R::ADDRESS], buffer)
            .await
            .map_err(Error::Transfer)
    }

    async fn write_register_async<R: Register<u8>>(&mut self, reg: R) -> Result<(), Self::Error> {
        self.write_sequential_async::<R>(&mut [reg.bits()]).await
    }

    async fn write_sequential_async<R: Register<u8>>(
        &mut self,
        buffer: &mut [u8],
    ) -> Result<(), Self::Error> {
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
    I2C: I2c,
{
    fn write_and_verify_register<R: Register<u8>>(
        &mut self,
        reg: R,
        delay: &mut impl embedded_hal::delay::DelayUs,
    ) -> Result<(), Error<I2C>> {
        for _ in 0..3 {
            self.write_sequential::<R>(&mut [reg.bits()])?;
            delay.delay_ms(1);
            let value = self.read_register::<R>()?;
            if value == reg {
                return Ok(());
            }
        }
        Err(Error::Verify)
    }
}

impl<I2C> Max17055<I2C>
where
    I2C: AsyncI2c,
{
    async fn write_and_verify_register_async<R: Register<u8>>(
        &mut self,
        reg: R,
        delay: &mut impl embedded_hal_async::delay::DelayUs,
    ) -> Result<(), Error<I2C>> {
        for _ in 0..3 {
            self.write_sequential_async::<R>(&mut [reg.bits()]).await?;
            delay.delay_ms(1).await;
            let value = self.read_register_async::<R>().await?;
            if value == reg {
                return Ok(());
            }
        }
        Err(Error::Verify)
    }
}
