#![no_std]

use embedded_hal_async::{
    delay::DelayNs as AsyncDelayNs,
    i2c::{ErrorType, I2c as AsyncI2c},
};

pub mod ll;

#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ConfigError<I>
where
    I: ErrorType,
{
    Transfer(I::Error),
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

#[derive(Clone, Copy, Default, Debug, PartialEq)]
//#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct LearnedParams {
    pub rcomp0: ll::Rcomp0FieldSet,
    pub temp_co: ll::TempCoFieldSet,
    pub full_cap_rep: ll::FullCapRepFieldSet,
    pub cycles: ll::CyclesFieldSet,
    pub full_cap_nom: ll::FullCapNomFieldSet,
}

pub struct Max17055<I> {
    driver: ll::Max17055<ll::Max17055I2cInterface<I>>,
    config: DesignData,
}

impl<I> Max17055<I> {
    pub const fn new(i2c: I, config: DesignData) -> Self {
        Self {
            driver: ll::Max17055::new(ll::Max17055I2cInterface { i2c }),
            config,
        }
    }
}

impl<I> Max17055<I>
where
    I: AsyncI2c,
{
    /// This function implements the Initialize Registers to Recommended Configuration
    /// procedure from the datasheet.
    pub async fn load_initial_config_async(
        &mut self,
        delay: &mut impl AsyncDelayNs,
    ) -> Result<(), ConfigError<I>> {
        if self
            .driver
            .status()
            .read_async()
            .await
            .map_err(ConfigError::Transfer)?
            .por()
            != ll::PowerOnReset::Reset
        {
            return Ok(());
        }

        while self
            .driver
            .fstat()
            .read_async()
            .await
            .map_err(ConfigError::Transfer)?
            .dnr()
            != ll::DataNotReady::Ready
        {
            delay.delay_ms(10).await;
        }

        let hib_cfg = self
            .force_exit_hiberation()
            .await
            .map_err(ConfigError::Transfer)?;

        self.ez_config(self.config)
            .await
            .map_err(ConfigError::Transfer)?;

        while self
            .driver
            .model_cfg()
            .read_async()
            .await
            .map_err(ConfigError::Transfer)?
            .refresh()
        {
            delay.delay_ms(10).await;
        }

        self.driver
            .hib_cfg()
            .write_async(|reg| *reg = hib_cfg)
            .await
            .map_err(ConfigError::Transfer)?;

        // Attempt tp clear POR flag
        let status = self
            .driver
            .status()
            .read_async()
            .await
            .map_err(ConfigError::Transfer)?;

        for _ in 0..3 {
            self.driver
                .status()
                .write_async(|reg| {
                    *reg = status;
                    reg.set_por(ll::PowerOnReset::NoReset);
                })
                .await
                .map_err(ConfigError::Transfer)?;

            if self
                .driver
                .status()
                .read_async()
                .await
                .map_err(ConfigError::Transfer)?
                .por()
                == ll::PowerOnReset::NoReset
            {
                return Ok(());
            }

            delay.delay_ms(1).await;
        }

        Err(ConfigError::Verify)
    }

    async fn force_exit_hiberation(&mut self) -> Result<ll::HibCfgFieldSet, I::Error> {
        let hib_cfg = self.driver.hib_cfg().read_async().await?;

        self.driver.soft_wakeup().dispatch_async().await?;

        self.driver
            .hib_cfg()
            .write_async(|reg| {
                reg.set_en_hib(false);
                reg.set_hib_config(0);
            })
            .await?;

        self.driver.clear().dispatch_async().await?;

        Ok(hib_cfg)
    }

    async fn ez_config(&mut self, config: DesignData) -> Result<(), I::Error> {
        const CHG_V_LOW: u32 = 44138;
        const CHG_V_HIGH: u32 = 51200;
        const CHG_THRESHOLD: u16 = 4275;

        let raw_capacity = config.uAh_to_raw_capacity(config.capacity as u32 * 1_000);

        self.driver
            .design_cap()
            .write_async(|reg| reg.set_capacity(raw_capacity))
            .await?;
        self.driver
            .d_qacc()
            .write_async(|reg| reg.set_capacity(raw_capacity / 32))
            .await?;
        self.driver
            .ichg_term()
            .write_async(|reg| {
                reg.set_current(config.i_chg_term as u16);
            })
            .await?;

        self.driver
            .vempty()
            .write_async(|reg| {
                reg.set_ve(config.v_empty / 10);
                reg.set_vr(config.v_recovery / 40);
            })
            .await?;

        let vchg = if config.v_charge > CHG_THRESHOLD {
            ll::Vchg::_4_4v
        } else {
            ll::Vchg::_4_2v
        };

        let dpacc = if vchg == ll::Vchg::_4_4v {
            (CHG_V_HIGH / 32) as u16
        } else {
            (CHG_V_LOW / 32) as u16
        };

        self.driver
            .d_pacc()
            .write_async(|reg| {
                reg.set_percentage(dpacc);
            })
            .await?;

        self.driver
            .model_cfg()
            .write_async(|reg| {
                reg.set_refresh(true);
                reg.set_v_chg(vchg);
                reg.set_model_id(ll::ModelId::Default);
            })
            .await?;

        Ok(())
    }

    /// Returns the reported capacity in μAh.
    pub async fn read_design_capacity(&mut self) -> Result<u32, I::Error> {
        let reg = self.driver.design_cap().read_async().await?;
        let raw = reg.capacity();
        Ok(self.config.raw_capacity_to_uAh(raw))
    }

    /// Returns the reported remaining capacity in μAh.
    pub async fn read_reported_remaining_capacity(&mut self) -> Result<u32, I::Error> {
        let reg = self.driver.rep_cap().read_async().await?;
        let raw = reg.capacity();
        Ok(self.config.raw_capacity_to_uAh(raw))
    }

    /// Returns the reported full capacity in μAh.
    pub async fn read_reported_capacity(&mut self) -> Result<u32, I::Error> {
        let reg = self.driver.full_cap_rep().read_async().await?;
        let raw = reg.capacity();
        Ok(self.config.raw_capacity_to_uAh(raw))
    }

    /// Returns the cell age in %.
    pub async fn read_cell_age(&mut self) -> Result<u8, I::Error> {
        let reg = self.driver.age().read_async().await?;
        let raw = reg.percentage();
        Ok((raw >> 8) as u8)
    }

    /// Returns the reported state of charge %.
    pub async fn read_reported_soc(&mut self) -> Result<u8, I::Error> {
        let reg = self.driver.rep_soc().read_async().await?;
        let raw = reg.percentage();
        Ok((raw >> 8) as u8)
    }

    /// Returns the number of charge cycles in %.
    pub async fn read_charge_cycles(&mut self) -> Result<u16, I::Error> {
        let reg = self.driver.cycles().read_async().await?;
        let raw = reg.cycles_percentage();
        Ok((raw / 100) as u16)
    }

    /// Returns the cell voltage in μV.
    pub async fn read_vcell(&mut self) -> Result<u32, I::Error> {
        let reg = self.driver.vcell().read_async().await?;
        let raw = reg.voltage();
        Ok(self.config.raw_voltage_to_uV(raw))
    }

    /// Returns the average cell voltage in μV.
    pub async fn read_avg_vcell(&mut self) -> Result<u32, I::Error> {
        let reg = self.driver.avg_vcell().read_async().await?;
        let raw = reg.voltage();
        Ok(self.config.raw_voltage_to_uV(raw))
    }

    /// Returns the battery current in μA.
    pub async fn read_current(&mut self) -> Result<i32, I::Error> {
        let reg = self.driver.current().read_async().await?;
        let taw = reg.current();
        Ok(self.config.raw_current_to_uA(taw))
    }

    /// Returns the average battery current in μA.
    pub async fn read_avg_current(&mut self) -> Result<i32, I::Error> {
        let reg = self.driver.avg_current().read_async().await?;
        let taw = reg.current();
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
    pub async fn read_learned_params(&mut self) -> Result<LearnedParams, I::Error> {
        Ok(LearnedParams {
            rcomp0: self.driver.rcomp0().read_async().await?,
            temp_co: self.driver.temp_co().read_async().await?,
            full_cap_rep: self.driver.full_cap_rep().read_async().await?,
            cycles: self.driver.cycles().read_async().await?,
            full_cap_nom: self.driver.full_cap_nom().read_async().await?,
        })
    }

    async fn attempt_restore_learned_params(
        &mut self,
        params: &LearnedParams,
        delay: &mut impl AsyncDelayNs,
    ) -> Result<(), I::Error> {
        self.driver
            .rcomp0()
            .write_async(|reg| *reg = params.rcomp0)
            .await?;

        self.driver
            .temp_co()
            .write_async(|reg| *reg = params.temp_co)
            .await?;
        self.driver
            .full_cap_nom()
            .write_async(|reg| *reg = params.full_cap_nom)
            .await?;

        delay.delay_ms(350).await;

        let full_cap_nom = self.driver.full_cap_nom().read_async().await?;
        let mixsoc = self.driver.mix_soc().read_async().await?;

        let mix_cap_calc = (mixsoc.percentage() as u32 * full_cap_nom.capacity() as u32) / 25600;

        self.driver
            .mix_cap()
            .write_async(|reg| reg.set_capacity(mix_cap_calc as u16))
            .await?;
        self.driver
            .full_cap_rep()
            .write_async(|reg| *reg = params.full_cap_rep)
            .await?;

        // 200%
        self.driver
            .d_pacc()
            .write_async(|reg| reg.set_percentage(0x0C80))
            .await?;
        self.driver
            .d_qacc()
            .write_async(|reg| reg.set_capacity(params.full_cap_nom.capacity() / 16))
            .await?;

        delay.delay_ms(350).await;

        self.driver
            .cycles()
            .write_async(|reg| *reg = params.cycles)
            .await?;

        Ok(())
    }

    /// Restore Parameters Function for battery Fuel Gauge model.
    ///
    /// If power is lost, then the capacity information can be easily restored with this function.
    pub async fn restore_learned_params(
        &mut self,
        params: &LearnedParams,
        delay: &mut impl AsyncDelayNs,
    ) -> Result<(), ConfigError<I>> {
        for _ in 0..3 {
            self.attempt_restore_learned_params(params, delay)
                .await
                .map_err(ConfigError::Transfer)?;
            let readback = self
                .read_learned_params()
                .await
                .map_err(ConfigError::Transfer)?;

            if readback == *params {
                return Ok(());
            }
            delay.delay_ms(1).await;
        }

        Err(ConfigError::Verify)
    }
}
