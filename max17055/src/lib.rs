#![no_std]
#![feature(async_fn_in_trait)]
#![allow(incomplete_features)]

use byte_slice_cast::{AsByteSlice, AsMutByteSlice};
use device_descriptor::{ReadOnlyRegister, Register};
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

#[derive(Clone, Copy)]
pub struct DesignData {
    /// Design capacity
    /// LSB = 5μVH/r_sense
    pub capacity: u16,

    /// The IChgTerm register allows the device to detect when a charge cycle of the cell has
    /// completed.
    /// LSB = 1.5625μV/r_sense
    pub i_chg_term: u16,

    /// Empty Voltage Target, During Load.
    /// LSB = 1mV
    pub v_empty: u16,

    /// Recovery voltage
    /// LSB = 40mV
    pub v_recovery: u16,

    /// Cell charged voltage
    /// LSB = 1mV
    pub v_charge: u16,

    /// LSB = 1mOhm
    pub r_sense: u32,
}

pub struct Max17055<I2C> {
    i2c: I2C,
    config: DesignData,
}

impl<I2C> RegisterAccess<u16> for Max17055<I2C>
where
    I2C: I2c,
{
    type Error = Error<I2C::Error>;

    fn read_register<R>(&mut self) -> Result<R, Self::Error>
    where
        R: ReadOnlyRegister<RegisterWidth = u16>,
    {
        let mut buffer = [0];
        self.read_sequential::<R>(&mut buffer)
            .map(|_| R::from_bits(buffer[0]))
    }

    fn read_sequential<R>(&mut self, buffer: &mut [u16]) -> Result<(), Self::Error>
    where
        R: ReadOnlyRegister<RegisterWidth = u16>,
    {
        for el in buffer.iter_mut() {
            *el = el.to_le();
        }
        self.i2c
            .write_read(Self::DEVICE_ADDR, &[R::ADDRESS], buffer.as_mut_byte_slice())
            .map_err(Error::Transfer)
    }

    fn write_register<R>(&mut self, reg: R) -> Result<(), Self::Error>
    where
        R: Register<RegisterWidth = u16>,
    {
        self.write_sequential::<R>(&mut [reg.bits()])
    }

    fn write_sequential<R>(&mut self, buffer: &mut [u16]) -> Result<(), Self::Error>
    where
        R: Register<RegisterWidth = u16>,
    {
        self.i2c
            .transaction(
                Self::DEVICE_ADDR,
                &mut [
                    Operation::Write(&[R::ADDRESS]),
                    Operation::Write(buffer.as_byte_slice()),
                ],
            )
            .map_err(Error::Transfer)?;

        for el in buffer.iter_mut() {
            *el = u16::from_le(*el);
        }

        Ok(())
    }
}

impl<I2C> AsyncRegisterAccess<u16> for Max17055<I2C>
where
    I2C: AsyncI2c,
{
    type Error = Error<I2C::Error>;

    async fn read_register_async<R>(&mut self) -> Result<R, Self::Error>
    where
        R: ReadOnlyRegister<RegisterWidth = u16>,
    {
        let mut buffer = [0];
        self.read_sequential_async::<R>(&mut buffer)
            .await
            .map(|_| R::from_bits(buffer[0]))
    }

    async fn read_sequential_async<R>(&mut self, buffer: &mut [u16]) -> Result<(), Self::Error>
    where
        R: ReadOnlyRegister<RegisterWidth = u16>,
    {
        for el in buffer.iter_mut() {
            *el = el.to_le();
        }
        self.i2c
            .write_read(Self::DEVICE_ADDR, &[R::ADDRESS], buffer.as_mut_byte_slice())
            .await
            .map_err(Error::Transfer)
    }

    async fn write_register_async<R>(&mut self, reg: R) -> Result<(), Self::Error>
    where
        R: Register<RegisterWidth = u16>,
    {
        self.write_sequential_async::<R>(&mut [reg.bits()]).await
    }

    async fn write_sequential_async<R>(&mut self, buffer: &mut [u16]) -> Result<(), Self::Error>
    where
        R: Register<RegisterWidth = u16>,
    {
        self.i2c
            .transaction(
                Self::DEVICE_ADDR,
                &mut [
                    Operation::Write(&[R::ADDRESS]),
                    Operation::Write(buffer.as_byte_slice()),
                ],
            )
            .await
            .map_err(Error::Transfer)?;

        for el in buffer.iter_mut() {
            *el = u16::from_le(*el);
        }

        Ok(())
    }
}

impl<I2C> Max17055<I2C> {
    const DEVICE_ADDR: u8 = 0x36; // or << 1

    pub fn new(i2c: I2C, config: DesignData) -> Self {
        Self { i2c, config }
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
        R: Register<RegisterWidth = u16>,
    {
        for _ in 0..3 {
            self.write_register_async(reg).await?;
            delay.delay_ms(1).await;
            let value = self.read_register_async::<R>().await?;
            if value.bits() == reg.bits() {
                return Ok(());
            }
        }
        Err(Error::Verify)
    }

    async fn poll_async<R: ReadOnlyRegister<RegisterWidth = u16>>(
        &mut self,
        delay: &mut impl AsyncDelayUs,
        predicate: impl Fn(&R) -> bool,
    ) -> Result<(), Error<I2C::Error>> {
        loop {
            let register = self.read_register_async::<R>().await?;
            if predicate(&register) {
                break;
            }

            delay.delay_ms(10).await;
        }

        Ok(())
    }

    /// This function implements the Initialize Registers to Recommended Configuration
    /// procedure from the datasheet.
    pub async fn load_initial_config_async(
        &mut self,
        delay: &mut impl AsyncDelayUs,
    ) -> Result<(), Error<I2C::Error>> {
        let por_status = self.read_register_async::<Status>().await?;
        if por_status.por().read() != Some(PowerOnReset::Reset) {
            return Ok(());
        }

        self.poll_async::<FStat>(delay, |reg| reg.dnr().read() == Some(DataNotReady::Ready))
            .await?;

        let hib_cfg = self.force_exit_hiberation().await?;

        self.ez_config(self.config).await?;
        self.poll_async::<ModelCfg>(delay, |reg| reg.refresh().read() == Some(Bit::NotSet))
            .await?;

        self.write_register_async(hib_cfg).await?;

        // Clear POR flag
        let status = self.read_register_async::<Status>().await?;
        self.write_and_verify_register_async(
            status.modify(|reg| reg.por().write(PowerOnReset::NoReset)),
            delay,
        )
        .await?;

        Ok(())
    }

    async fn force_exit_hiberation(&mut self) -> Result<HibCfg, Error<I2C::Error>> {
        let hib_cfg = self.read_register_async::<HibCfg>().await?;

        self.write_register_async(Command::new(|w| w.command().write(CommandKind::SoftWakeup)))
            .await?;

        self.write_register_async(HibCfg::new(|w| {
            w.en_hib().write(Bit::NotSet).hib_config().write(0)
        }))
        .await?;

        self.write_register_async(Command::new(|w| w.command().write(CommandKind::Clear)))
            .await?;

        Ok(hib_cfg)
    }

    async fn ez_config(&mut self, config: DesignData) -> Result<(), Error<I2C::Error>> {
        const CHG_V_LOW: u32 = 44138;
        const CHG_V_HIGH: u32 = 51200;
        const CHG_THRESHOLD: u16 = 4275;

        self.write_register_async(DesignCap::new(|w| w.capacity().write(config.capacity)))
            .await?;
        self.write_register_async(dQAcc::new(|w| w.capacity().write(config.capacity / 32)))
            .await?;
        self.write_register_async(IChgTerm::new(|w| w.current().write(config.i_chg_term)))
            .await?;
        self.write_register_async(VEmpty::new(|w| {
            w.ve().write(config.v_empty).vr().write(config.v_recovery)
        }))
        .await?;

        let cap = config.capacity as u32;

        if config.v_charge > CHG_THRESHOLD {
            self.write_register_async(dPAcc::new(|w| {
                w.percentage().write((cap / 32 * CHG_V_HIGH / cap) as u16)
            }))
            .await?;
            self.write_register_async(ModelCfg::new(|w| {
                w.refresh()
                    .write(Bit::Set)
                    .v_chg()
                    .write(VChg::_4_4V)
                    .model_id()
                    .write(ModelID::Default)
            }))
            .await?;
        } else {
            self.write_register_async(dPAcc::new(|w| {
                w.percentage().write((cap / 32 * CHG_V_LOW / cap) as u16)
            }))
            .await?;
            self.write_register_async(ModelCfg::new(|w| {
                w.refresh()
                    .write(Bit::Set)
                    .v_chg()
                    .write(VChg::_4_2V)
                    .model_id()
                    .write(ModelID::Default)
            }))
            .await?;
        }

        Ok(())
    }
}