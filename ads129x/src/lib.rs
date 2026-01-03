#![no_std]

use embedded_hal::digital::OutputPin;
use embedded_hal_async::spi::{ErrorType, Operation, SpiDevice as AsyncSpiDevice};

pub mod ll;

// t_mod = 1/128kHz
const MIN_T_POR: u32 = 32; // >= 4096 * t_mod >= 1/32s
const MIN_T_RST: u32 = 1; // >= 1 * t_mod >= 8us
const MIN_RST_WAIT: u32 = 1; // >= 18 * t_mod >= 140us

pub struct Ads129x<S> {
    driver: ll::Ads129X<ll::Ads129xSpiInterface<S>>,
}

impl<S> Ads129x<S> {
    pub const fn new(spi_device: S) -> Self {
        Self {
            driver: ll::Ads129X::new(ll::Ads129xSpiInterface { spi: spi_device }),
        }
    }

    pub fn inner_mut(&mut self) -> &mut S {
        &mut self.driver.interface().spi
    }

    pub fn into_inner(self) -> S {
        self.driver.interface.spi
    }
}

impl<S> Ads129x<S>
where
    S: AsyncSpiDevice,
{
    pub async fn pulse_reset_async<RESET>(
        &mut self,
        reset: &mut RESET,
        delay: &mut impl embedded_hal_async::delay::DelayNs,
    ) -> Result<(), RESET::Error>
    where
        RESET: OutputPin,
    {
        reset.set_high()?;
        delay.delay_ms(MIN_T_POR).await;
        reset.set_low()?;
        delay.delay_ms(MIN_T_RST).await;
        reset.set_high()?;
        delay.delay_ms(MIN_RST_WAIT).await;

        Ok(())
    }

    pub async fn start_command_async(&mut self) -> Result<(), S::Error> {
        self.driver.start().dispatch_async().await
    }

    pub async fn rdatac_command_async(&mut self) -> Result<(), S::Error> {
        self.driver.rdata_c().dispatch_async().await
    }

    pub async fn sdatac_command_async(&mut self) -> Result<(), S::Error> {
        self.driver.sdata_c().dispatch_async().await
    }

    pub async fn stop_command_async(&mut self) -> Result<(), S::Error> {
        self.driver.stop().dispatch_async().await
    }

    pub async fn reset_command_async(&mut self) -> Result<(), S::Error> {
        self.driver.reset().dispatch_async().await
    }

    pub async fn change_clock_divider_async(
        &mut self,
        divider: ll::ClockDivider,
    ) -> Result<(), S::Error> {
        self.driver
            .loff_stat()
            .modify_async(|reg| reg.set_clk_div(divider))
            .await
    }

    pub async fn read_sample_async(&mut self) -> Result<AdsData, S::Error> {
        let data = self.driver.rdata().dispatch_async().await?;
        Ok(AdsData { raw: data })
    }

    pub async fn read_continuous_sample_async(&mut self) -> Result<AdsData, S::Error> {
        let mut bytes = [0; _];
        self.driver
            .interface()
            .spi
            .transaction(&mut ll::ops!(rdatac, &mut bytes))
            .await?;
        Ok(AdsData {
            raw: ll::RdataFieldSetOut::from(bytes),
        })
    }

    pub async fn read_gpio_async(&mut self) -> Result<ll::GpioFieldSet, S::Error> {
        self.driver.gpio().read_async().await
    }

    pub async fn write_gpio_async(&mut self, register: ll::GpioFieldSet) -> Result<(), S::Error> {
        self.driver.gpio().write_async(|reg| *reg = register).await
    }

    pub async fn read_device_id_async(&mut self) -> Result<ll::IdFieldSet, S::Error> {
        self.driver.id().read_async().await
    }

    pub async fn apply_config_async(
        &mut self,
        mut config: ConfigRegisters,
    ) -> Result<(), AdsConfigError<S>> {
        self.write_config_async(&config)
            .await
            .map_err(AdsConfigError::Spi)?;
        let mut readback = self
            .read_config_async()
            .await
            .map_err(AdsConfigError::Spi)?;

        config.mask_off_status_bits();
        readback.mask_off_status_bits();

        if readback != config {
            #[cfg(feature = "defmt")]
            defmt::warn!("Config mismatch: {:?} != {:?}", readback, config);
            return Err(AdsConfigError::ReadbackMismatch);
        }

        Ok(())
    }

    async fn write_config_async(&mut self, config: &ConfigRegisters) -> Result<(), S::Error> {
        self.driver
            .config1()
            .write_async(|reg| *reg = config.config1)
            .await?;
        self.driver
            .config2()
            .write_async(|reg| *reg = config.config2)
            .await?;

        self.driver
            .loff()
            .write_async(|reg| *reg = config.loff)
            .await?;

        self.driver
            .ch1set()
            .write_async(|reg| *reg = config.ch1set)
            .await?;
        self.driver
            .ch2set()
            .write_async(|reg| *reg = config.ch2set)
            .await?;

        self.driver
            .rld_sens()
            .write_async(|reg| *reg = config.rldsens)
            .await?;
        self.driver
            .loff_sens()
            .write_async(|reg| *reg = config.loffsens)
            .await?;
        self.driver
            .loff_stat()
            .write_async(|reg| *reg = config.loffstat)
            .await?;

        self.driver
            .resp1()
            .write_async(|reg| *reg = config.resp1)
            .await?;
        self.driver
            .resp2()
            .write_async(|reg| *reg = config.resp2)
            .await?;

        self.driver
            .gpio()
            .write_async(|reg| *reg = config.gpio)
            .await?;

        Ok(())
    }

    async fn read_config_async(&mut self) -> Result<ConfigRegisters, S::Error> {
        Ok(ConfigRegisters {
            config1: self.driver.config1().read_async().await?,
            config2: self.driver.config2().read_async().await?,
            loff: self.driver.loff().read_async().await?,
            ch1set: self.driver.ch1set().read_async().await?,
            ch2set: self.driver.ch2set().read_async().await?,
            rldsens: self.driver.rld_sens().read_async().await?,
            loffsens: self.driver.loff_sens().read_async().await?,
            loffstat: self.driver.loff_stat().read_async().await?,
            resp1: self.driver.resp1().read_async().await?,
            resp2: self.driver.resp2().read_async().await?,
            gpio: self.driver.gpio().read_async().await?,
        })
    }
}

// Blocking implementations

#[derive(Copy, Clone, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum AdsConfigError<S>
where
    S: ErrorType,
{
    ReadbackMismatch,

    Spi(S::Error),
}

// TODO: should not expose raw register types?

#[derive(Copy, Clone, Default, Debug, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct ConfigRegisters {
    pub config1: ll::Config1FieldSet,
    pub config2: ll::Config2FieldSet,
    pub loff: ll::LoffFieldSet,
    pub ch1set: ll::Ch1setFieldSet,
    pub ch2set: ll::Ch2setFieldSet,
    pub rldsens: ll::RldSensFieldSet,
    pub loffsens: ll::LoffSensFieldSet,
    pub loffstat: ll::LoffStatFieldSet,
    pub resp1: ll::Resp1FieldSet,
    pub resp2: ll::Resp2FieldSet,
    pub gpio: ll::GpioFieldSet,
}
impl ConfigRegisters {
    fn mask_off_status_bits(&mut self) {
        self.loffstat.set_rld(ll::LeadStatus::Connected);
        self.loffstat.set_in2n(ll::LeadStatus::Connected);
        self.loffstat.set_in2p(ll::LeadStatus::Connected);
        self.loffstat.set_in1n(ll::LeadStatus::Connected);
        self.loffstat.set_in1p(ll::LeadStatus::Connected);

        self.gpio.set_d2(ll::PinState::Low);
        self.gpio.set_d1(ll::PinState::Low);
    }
}

#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct AdsData {
    raw: ll::RdataFieldSetOut,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Sample {
    sample: i32,
}

impl AdsData {
    #[inline]
    pub fn new(buffer: [u8; 9]) -> Self {
        Self {
            raw: ll::RdataFieldSetOut::from(buffer),
        }
    }

    #[inline]
    pub fn ch1_positive_lead_connected(&self) -> bool {
        self.raw.in1p() == ll::LeadStatus::Connected
    }

    #[inline]
    pub fn ch1_negative_lead_connected(&self) -> bool {
        self.raw.in1n() == ll::LeadStatus::Connected
    }

    #[inline]
    pub fn ch1_leads_connected(&self) -> bool {
        self.ch1_negative_lead_connected() && self.ch1_positive_lead_connected()
    }

    #[inline]
    pub fn ch2_positive_lead_connected(&self) -> bool {
        self.raw.in2p() == ll::LeadStatus::Connected
    }

    #[inline]
    pub fn ch2_negative_lead_connected(&self) -> bool {
        self.raw.in2n() == ll::LeadStatus::Connected
    }

    #[inline]
    pub fn ch2_leads_connected(&self) -> bool {
        self.ch2_negative_lead_connected() && self.ch2_positive_lead_connected()
    }

    #[inline]
    pub fn ch1_sample(&self) -> Sample {
        Sample {
            sample: self.raw.ch1(),
        }
    }

    #[inline]
    pub fn ch2_sample(&self) -> Sample {
        Sample {
            sample: self.raw.ch2(),
        }
    }
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
