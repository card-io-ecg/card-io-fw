#![no_std]
#![feature(async_fn_in_trait)]
#![allow(stable_features, unknown_lints, async_fn_in_trait)]

#[macro_use]
extern crate logger;

use byte_slice_cast::AsMutByteSlice;
use device_descriptor::{ReadOnlyRegister, ReaderProxy, Register};
use embedded_hal::i2c::I2c;
use embedded_hal_async::{delay::DelayUs as AsyncDelayUs, i2c::I2c as AsyncI2c};
use register_access::{AsyncRegisterAccess, RegisterAccess};

use crate::descriptors::*;

pub mod descriptors;

#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error<I2cE> {
    Transfer(I2cE),
    Verify,
}

#[derive(Clone, Copy, Default, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct DesignData {
    /// Design capacity
    /// LSB = 5μVH/r_sense
    pub capacity: u16,

    /// The IChgTerm register allows the device to detect when a charge cycle of the cell has
    /// completed.
    /// LSB = 1.5625μV/r_sense
    pub i_chg_term: i16,

    /// Empty Voltage Target, During Load.
    /// LSB = 1mV
    pub v_empty: u16,

    /// Recovery voltage
    /// LSB = 1mV
    pub v_recovery: u16,

    /// Cell charged voltage
    /// LSB = 1mV
    pub v_charge: u16,

    /// LSB = 1mOhm
    pub r_sense: u32,
}

impl DesignData {
    /// Converts the raw register value to a current value in μA.
    ///
    /// ```rust
    /// # use max17055::DesignData;
    /// let design_data = DesignData {
    ///    r_sense: 20,
    ///   ..Default::default()
    /// };
    ///
    /// assert_eq!(design_data.raw_current_to_uA(0), 0);
    /// assert_eq!(design_data.raw_current_to_uA(1), 78);
    /// assert_eq!(design_data.raw_current_to_uA(0xFFFF), -78);
    ///
    /// let design_data = DesignData {
    ///    r_sense: 10,
    ///   ..Default::default()
    /// };
    ///
    /// assert_eq!(design_data.raw_current_to_uA(0), 0);
    /// assert_eq!(design_data.raw_current_to_uA(1), 156);
    /// assert_eq!(design_data.raw_current_to_uA(0xFFFF), -156);
    /// ```
    #[allow(non_snake_case)]
    #[inline]
    pub fn raw_current_to_uA(&self, raw: u16) -> i32 {
        let raw = raw as i16 as i32;
        let rsense = self.r_sense as i32;

        (raw * 1_5625) / (rsense * 10)
    }

    /// Converts the raw register value to a capacity value in μAh.
    ///
    /// ```rust
    /// # use max17055::DesignData;
    /// let design_data = DesignData {
    ///    r_sense: 20,
    ///   ..Default::default()
    /// };
    ///
    /// assert_eq!(design_data.raw_capacity_to_uAh(0), 0);
    /// assert_eq!(design_data.raw_capacity_to_uAh(1), 250);
    /// assert_eq!(design_data.raw_capacity_to_uAh(65535), 16_383_750);
    /// ```
    #[allow(non_snake_case)]
    #[inline]
    pub fn raw_capacity_to_uAh(&self, raw: u16) -> u32 {
        let raw = raw as u32;
        let rsense = self.r_sense;

        (raw * 5_000) / rsense
    }

    /// Converts the raw register value to a capacity value in μAh.
    ///
    /// ```rust
    /// # use max17055::DesignData;
    /// let design_data = DesignData {
    ///    r_sense: 20,
    ///   ..Default::default()
    /// };
    ///
    /// assert_eq!(design_data.uAh_to_raw_capacity(0), 0);
    /// assert_eq!(design_data.uAh_to_raw_capacity(250), 1);
    /// assert_eq!(design_data.uAh_to_raw_capacity(16_383_750), 65535);
    /// ```
    #[allow(non_snake_case)]
    #[inline]
    pub fn uAh_to_raw_capacity(&self, uah: u32) -> u16 {
        (uah * self.r_sense / 5_000) as u16
    }

    /// Converts the raw register value to a voltage value in μV.
    ///
    /// ```rust
    /// # use max17055::DesignData;
    /// let design_data = DesignData {
    ///   r_sense: 20,
    ///  ..Default::default()
    /// };
    ///
    /// assert_eq!(design_data.raw_voltage_to_uV(0), 0);
    /// assert_eq!(design_data.raw_voltage_to_uV(1), 78);
    /// assert_eq!(design_data.raw_voltage_to_uV(65535), 5_119_921);
    /// ```
    #[allow(non_snake_case)]
    #[inline]
    pub fn raw_voltage_to_uV(&self, raw: u16) -> u32 {
        let raw = raw as u32;

        (raw * 625) / 8
    }
}

pub struct LearnedParams {
    pub rcomp0: u16,
    pub temp_co: u16,
    pub full_cap_rep: u16,
    pub cycles: u16,
    pub full_cap_nom: u16,
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
        self.i2c
            .write_read(Self::DEVICE_ADDR, &[R::ADDRESS], buffer.as_mut_byte_slice())
            .map_err(Error::Transfer)?;

        for el in buffer.iter_mut() {
            *el = u16::from_le(*el);
        }

        Ok(())
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
        for (i, el) in buffer.iter_mut().enumerate() {
            self.write_one((R::ADDRESS as usize + i) as u8, *el)?;
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
        self.i2c
            .write_read(Self::DEVICE_ADDR, &[R::ADDRESS], buffer.as_mut_byte_slice())
            .await
            .map_err(Error::Transfer)?;

        for el in buffer.iter_mut() {
            *el = u16::from_le(*el);
        }

        Ok(())
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
        for (i, el) in buffer.iter_mut().enumerate() {
            self.write_one_async((R::ADDRESS as usize + i) as u8, *el)
                .await?;
        }

        Ok(())
    }
}

impl<I2C> Max17055<I2C> {
    const DEVICE_ADDR: u8 = 0x36;

    pub fn new(i2c: I2C, config: DesignData) -> Self {
        debug!("Design data: {:?}", config);
        Self { i2c, config }
    }

    fn write_register_data(addr: u8, value: u16) -> [u8; 3] {
        let [lower, upper] = value.to_le_bytes();
        [addr, lower, upper]
    }
}

impl<I2C> Max17055<I2C>
where
    I2C: I2c,
{
    fn write_one(&mut self, addr: u8, value: u16) -> Result<(), Error<I2C::Error>> {
        let data = Self::write_register_data(addr, value);
        self.i2c
            .write(Self::DEVICE_ADDR, &data)
            .map_err(Error::Transfer)
    }
}

impl<I2C> Max17055<I2C>
where
    I2C: AsyncI2c,
{
    async fn write_one_async(&mut self, addr: u8, value: u16) -> Result<(), Error<I2C::Error>> {
        let data = Self::write_register_data(addr, value);
        self.i2c
            .write(Self::DEVICE_ADDR, &data)
            .await
            .map_err(Error::Transfer)
    }

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
        trace!("Loading initial configuration");

        let por_status = match self.read_register_async::<Status>().await {
            Ok(por_status) => por_status,
            Err(e) => {
                error!("Failed to read status register");
                return Err(e);
            }
        };

        if por_status.por().read() != Some(PowerOnReset::Reset) {
            debug!("No power-on reset");
            return Ok(());
        }

        debug!("Power-on reset, initializing");
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

        let raw_capacity = config.uAh_to_raw_capacity(config.capacity as u32 * 1_000);

        self.write_register_async(DesignCap::new(|w| w.capacity().write(raw_capacity)))
            .await?;
        self.write_register_async(dQAcc::new(|w| w.capacity().write(raw_capacity / 32)))
            .await?;
        self.write_register_async(IChgTerm::new(|w| {
            w.current().write(config.i_chg_term as u16)
        }))
        .await?;
        self.write_register_async(VEmpty::new(|w| {
            w.ve()
                .write(config.v_empty / 10)
                .vr()
                .write(config.v_recovery / 40)
        }))
        .await?;

        if config.v_charge > CHG_THRESHOLD {
            debug!("Configuring 4.4V battery");
            self.write_register_async(dPAcc::new(|w| {
                w.percentage().write((CHG_V_HIGH / 32) as u16)
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
            debug!("Configuring 4.2V battery");
            self.write_register_async(dPAcc::new(|w| {
                w.percentage().write((CHG_V_LOW / 32) as u16)
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

    /// Returns the reported capacity in μAh.
    pub async fn read_design_capacity(&mut self) -> Result<u32, Error<I2C::Error>> {
        let reg = self.read_register_async::<DesignCap>().await?;
        let raw = reg.capacity().read().unwrap_or(0);
        Ok(self.config.raw_capacity_to_uAh(raw))
    }

    /// Returns the reported remaining capacity in μAh.
    pub async fn read_reported_remaining_capacity(&mut self) -> Result<u32, Error<I2C::Error>> {
        let reg = self.read_register_async::<RepCap>().await?;
        let raw = reg.capacity().read().unwrap_or(0);
        Ok(self.config.raw_capacity_to_uAh(raw))
    }

    /// Returns the reported full capacity in μAh.
    pub async fn read_reported_capacity(&mut self) -> Result<u32, Error<I2C::Error>> {
        let reg = self.read_register_async::<FullCapRep>().await?;
        let raw = reg.capacity().read().unwrap_or(0);
        Ok(self.config.raw_capacity_to_uAh(raw))
    }

    /// Returns the cell age in %.
    pub async fn read_cell_age(&mut self) -> Result<u8, Error<I2C::Error>> {
        let reg = self.read_register_async::<Age>().await?;
        let raw = reg.percentage().read().unwrap_or(0);
        Ok((raw >> 8) as u8)
    }

    /// Returns the reported state of charge %.
    pub async fn read_reported_soc(&mut self) -> Result<u8, Error<I2C::Error>> {
        let reg = self.read_register_async::<RepSOC>().await?;
        let raw = reg.percentage().read().unwrap_or(0);
        Ok((raw >> 8) as u8)
    }

    /// Returns the number of charge cycles in %.
    pub async fn read_charge_cycles(&mut self) -> Result<u16, Error<I2C::Error>> {
        let reg = self.read_register_async::<Cycles>().await?;
        let raw = reg.cycles_percentage().read().unwrap_or(0);
        Ok((raw / 100) as u16)
    }

    /// Returns the cell voltage in μV.
    pub async fn read_vcell(&mut self) -> Result<u32, Error<I2C::Error>> {
        let reg = self.read_register_async::<VCell>().await?;
        let raw = reg.voltage().read().unwrap_or(0);
        Ok(self.config.raw_voltage_to_uV(raw))
    }

    /// Returns the average cell voltage in μV.
    pub async fn read_avg_vcell(&mut self) -> Result<u32, Error<I2C::Error>> {
        let reg = self.read_register_async::<AvgVCell>().await?;
        let raw = reg.voltage().read().unwrap_or(0);
        Ok(self.config.raw_voltage_to_uV(raw))
    }

    /// Returns the battery current in μA.
    pub async fn read_current(&mut self) -> Result<i32, Error<I2C::Error>> {
        let reg = self.read_register_async::<Current>().await?;
        let taw = reg.current().read().unwrap_or(0);
        Ok(self.config.raw_current_to_uA(taw))
    }

    /// Returns the average battery current in μA.
    pub async fn read_avg_current(&mut self) -> Result<i32, Error<I2C::Error>> {
        let reg = self.read_register_async::<AvgCurrent>().await?;
        let taw = reg.current().read().unwrap_or(0);
        Ok(self.config.raw_current_to_uA(taw))
    }

    /// Save Learned Parameters Function for battery Fuel Gauge model.
    ///
    /// It is recommended to save the learned capacity parameters every
    /// time bit 2 of the Cycles register toggles
    /// (so that it is saved every 64% change in the battery)
    /// so that if power is lost the values can easily be restored. Make sure
    /// the data is saved on a non-volatile memory. Call this function after first initialization
    /// for reference in future function calls.
    /// Max number of cycles is 655.35 cycles with a LSB of 1% for the cycles register
    pub async fn read_learned_params(&mut self) -> Result<LearnedParams, Error<I2C::Error>> {
        Ok(LearnedParams {
            rcomp0: self.read_register_async::<RComp0>().await?.value(),
            temp_co: self.read_register_async::<TempCo>().await?.value(),
            full_cap_rep: self.read_register_async::<FullCap>().await?.value(),
            cycles: self.read_register_async::<Cycles>().await?.value(),
            full_cap_nom: self.read_register_async::<FullCapNom>().await?.value(),
        })
    }

    /// Restore Parameters Function for battery Fuel Gauge model.
    ///
    /// If power is lost, then the capacity information can be easily restored with this function.
    pub async fn restore_learned_params(
        &mut self,
        params: &LearnedParams,
        delay: &mut impl AsyncDelayUs,
    ) -> Result<(), Error<I2C::Error>> {
        self.write_and_verify_register_async(RComp0::from_bits(params.rcomp0), delay)
            .await?;
        self.write_and_verify_register_async(TempCo::from_bits(params.temp_co), delay)
            .await?;
        self.write_and_verify_register_async(FullCapNom::from_bits(params.full_cap_nom), delay)
            .await?;
        delay.delay_ms(350).await;

        let full_cap_nom = self.read_register_async::<FullCapNom>().await?;
        let mixsoc = self.read_register_async::<MixSOC>().await?;

        let mix_cap_calc = (mixsoc.percentage().read_field_bits() as u32
            * full_cap_nom.capacity().read_field_bits() as u32)
            / 25600;

        self.write_and_verify_register_async(MixCap::from_bits(mix_cap_calc as u16), delay)
            .await?;
        self.write_and_verify_register_async(FullCapRep::from_bits(params.full_cap_rep), delay)
            .await?;

        self.write_and_verify_register_async(dPAcc::from_bits(0x0C80), delay)
            .await?; // 200%
        self.write_and_verify_register_async(dQAcc::from_bits(params.full_cap_nom / 16), delay)
            .await?;

        delay.delay_ms(350).await;

        self.write_and_verify_register_async(Cycles::from_bits(params.cycles), delay)
            .await?;

        Ok(())
    }
}
