use ads129x::{ll, Ads129x, AdsConfigError, AdsData, ConfigRegisters};
use embassy_time::{Delay, Duration, Timer};
use embedded_hal::digital::{InputPin, OutputPin};
use embedded_hal_async::{digital::Wait, spi::SpiDevice as AsyncSpiDevice};

pub struct Frontend<S, I, O> {
    adc: Ads129x<S>,
    drdy: I,
    reset: O,
    // External clock enable pin. Optional if the RESET pin is used to control the oscillator, too.
    // TODO: verify power consumption. v1r7 devices have a
    // common RESET/CLK enable pin, will it prevent low power
    // mode with external oscillator? Modify CLKSEL so that a
    // GPIO can write it to enable external clock by pulling it low?
    clken: Option<O>,
    touch: I,
    device_id: Option<ll::DeviceId>,
}

impl<S, I, O> Frontend<S, I, O> {
    pub const fn new(spi: S, drdy: I, reset: O, clken: Option<O>, touch: I) -> Self {
        Self {
            adc: Ads129x::new(spi),
            drdy,
            reset,
            clken,
            touch,
            device_id: None,
        }
    }

    pub fn spi_mut(&mut self) -> &mut S {
        self.adc.inner_mut()
    }

    pub fn device_id(&self) -> Option<ll::DeviceId> {
        self.device_id
    }
}

impl<S, I, O> Frontend<S, I, O>
where
    S: AsyncSpiDevice,
    I: InputPin + Wait,
    O: OutputPin,
{
    fn config(&self) -> ConfigRegisters {
        ConfigRegisters {
            config1: {
                let mut r = ll::Config1fieldSet::new();
                r.set_data_rate(ll::DataRate::_1ksps);
                r.set_sampling(ll::Sampling::Continuous);
                r
            },

            config2: {
                let mut r = ll::Config2fieldSet::new();
                r.set_pdb_loff_comp(ll::Buffer::Enabled);
                r.set_ref_voltage(ll::ReferenceVoltage::_2_42v);
                r.set_clock_pin(ll::ClockPin::Disabled);
                r.set_test_signal(ll::TestSignal::Disabled);
                r
            },

            loff: {
                let mut r = ll::LoffFieldSet::new();
                r.set_comp_th(ll::ComparatorThreshold::_95);
                r.set_leadoff_current(ll::LeadOffCurrent::_22nA);
                r.set_leadoff_frequency(ll::LeadOffFrequency::Dc);
                r
            },

            ch1set: {
                let mut r = ll::Ch1setFieldSet::new();
                r.set_enabled(ll::Channel::Enabled);
                r.set_gain(ll::Gain::X1);
                r.set_mux(ll::Ch1mux::Normal);
                r
            },

            ch2set: {
                let mut r = ll::Ch2setFieldSet::new();
                r.set_enabled(ll::Channel::PowerDown);
                r.set_gain(ll::Gain::X1);
                r.set_mux(ll::Ch2mux::Shorted);
                r
            },

            rldsens: {
                let mut r = ll::RldSensFieldSet::new();
                r.set_chop(ll::ChopFrequency::Fmod2);
                r.set_pdb_rld(ll::Buffer::Enabled);
                r.set_loff_sense(ll::Input::NotConnected);
                r.set_rld2n(ll::Input::NotConnected);
                r.set_rld2p(ll::Input::NotConnected);
                r.set_rld1n(ll::Input::Connected);
                r.set_rld1p(ll::Input::Connected);
                r
            },

            loffsens: {
                let mut r = ll::LoffSensFieldSet::new();
                r.set_flip2(ll::CurrentDirection::Normal);
                r.set_flip1(ll::CurrentDirection::Normal);
                r.set_loff2n(ll::Input::NotConnected);
                r.set_loff2p(ll::Input::NotConnected);
                r.set_loff1n(ll::Input::Connected);
                r.set_loff1p(ll::Input::Connected);
                r
            },

            loffstat: {
                let mut r = ll::LoffStatFieldSet::new();
                r.set_clk_div(ll::ClockDivider::External512kHz);
                r
            },

            resp1: ll::Resp1fieldSet::default(),
            resp2: {
                let mut r = ll::Resp2fieldSet::new();
                r.set_rld_reference(ll::RldReference::MidSupply);
                r
            },

            gpio: {
                let mut r = ll::GpioFieldSet::new();
                r.set_c2(ll::PinDirection::Input);
                r.set_c1(ll::PinDirection::Output);
                r.set_d1(ll::PinState::High); // disable touch detector circuitry
                r
            },
        }
    }

    pub async fn enable_async(
        self,
        config: impl Fn(&mut ConfigRegisters),
    ) -> Result<PoweredFrontend<S, I, O>, (Self, AdsConfigError<S>)> {
        let mut frontend = PoweredFrontend {
            frontend: self,
            touched: true,
        };

        match frontend.enable(config).await {
            Ok(_) => Ok(frontend),
            Err(err) => Err((frontend.shut_down().await, err)),
        }
    }

    pub fn is_touched(&mut self) -> bool {
        unwrap!(self.touch.is_low().ok())
    }

    pub async fn wait_for_release(&mut self) {
        unwrap!(self.touch.wait_for_high().await.ok());
    }

    pub fn split(self) -> (S, I, O, I) {
        (self.adc.into_inner(), self.drdy, self.reset, self.touch)
    }
}

pub struct PoweredFrontend<S, I, O> {
    frontend: Frontend<S, I, O>,
    touched: bool,
}

impl<S, I, O> PoweredFrontend<S, I, O>
where
    S: AsyncSpiDevice,
    I: InputPin,
    O: OutputPin,
{
    pub fn spi_mut(&mut self) -> &mut S {
        self.frontend.spi_mut()
    }
}

impl<S, I, O> PoweredFrontend<S, I, O>
where
    S: AsyncSpiDevice,
    I: InputPin + Wait,
    O: OutputPin,
{
    async fn enable(
        &mut self,
        config: impl Fn(&mut ConfigRegisters),
    ) -> Result<(), AdsConfigError<S>> {
        // Enable external clock if it is separately controlled.
        if let Some(clken) = self.frontend.clken.as_mut() {
            unwrap!(clken.set_high().ok());
        }

        Timer::after(Duration::from_millis(1)).await;

        self.frontend
            .adc
            .pulse_reset_async(&mut self.frontend.reset, &mut Delay)
            .await
            .unwrap();

        // Exit RDATAC so that the device does not ignore our commands.
        self.frontend
            .adc
            .sdatac_command_async()
            .await
            .map_err(AdsConfigError::Spi)?;

        let device_id = self
            .frontend
            .adc
            .read_device_id_async()
            .await
            .map_err(AdsConfigError::Spi)?;

        match device_id.device_id() {
            Ok(device_id) => {
                info!("ADC device id: {:?}", device_id);
                self.frontend.device_id = Some(device_id);
            }
            Err(e) => {
                warn!("Failed to read ADC device id: {:?}", e);
                return Err(AdsConfigError::ReadbackMismatch);
            }
        }

        let mut config_regs = self.frontend.config();
        config(&mut config_regs);
        self.frontend.adc.apply_config_async(config_regs).await?;

        Ok(())
    }

    pub async fn start(&mut self) -> Result<(), S::Error> {
        self.frontend.adc.start_command_async().await
    }

    pub async fn set_clock_source(&mut self, use_external_clock: bool) -> Result<bool, S::Error> {
        let clksel = self
            .read_clksel()
            .await
            .inspect_err(|_| warn!("Failed to read CLKSEL"))?;

        let is_new_hardware = self.frontend.clken.is_none();

        let has_external_clock = if !is_new_hardware {
            info!("Hardware uses old frontend");
            // Separate CLK_EN and RESET pins, old module. External oscillator is present
            // if GPIO2 reads low.
            clksel == ll::PinState::Low
        } else {
            info!("Hardware uses new frontend");
            // Separate CLK_EN and RESET pins, new module. External oscillator is present
            // if GPIO2 reads high. External oscillator can be enabled by pulling GPIO2 low.
            clksel == ll::PinState::High
        };

        info!("Hardware has external clock: {:?}", has_external_clock);

        let enable_fast_clk = has_external_clock & use_external_clock;

        if enable_fast_clk {
            if is_new_hardware {
                info!("Setting GPIO2 to low on new hardware");
                let mut register = self.frontend.adc.read_gpio_async().await?;
                register.set_c2(ll::PinDirection::Output);
                register.set_d2(ll::PinState::Low);
                self.frontend.adc.write_gpio_async(register).await?;
            }

            info!("Enabling faster clock speeds");
            self.enable_fast_clock().await?;
        }

        Ok(enable_fast_clk)
    }

    #[allow(unused)]
    pub async fn enable_rdatac(&mut self) -> Result<(), S::Error> {
        self.frontend.adc.rdatac_command_async().await
    }

    pub async fn read_clksel(&mut self) -> Result<ll::PinState, S::Error> {
        let register = self.frontend.adc.read_gpio_async().await?;
        Ok(register.d2())
    }

    pub async fn enable_fast_clock(&mut self) -> Result<(), S::Error> {
        self.frontend
            .adc
            .change_clock_divider_async(ll::ClockDivider::External2mhz)
            .await
    }

    pub async fn read(&mut self) -> Result<AdsData, S::Error> {
        unwrap!(self.frontend.drdy.wait_for_falling_edge().await.ok());

        let sample = self.frontend.adc.read_sample_async().await?;
        self.touched = sample.ch1_negative_lead_connected();

        Ok(sample)
    }

    pub fn is_touched(&self) -> bool {
        self.touched
    }

    pub async fn shut_down(mut self) -> Frontend<S, I, O> {
        let _ = self.frontend.adc.stop_command_async().await;
        let _ = self.frontend.adc.reset_command_async().await;

        unwrap!(self.frontend.reset.set_low().ok());

        if let Some(clken) = self.frontend.clken.as_mut() {
            // Datasheet says to wait 2^10 clock cycles to enter power down mode. We give it a bit of
            // extra time.
            Timer::after(Duration::from_millis(5)).await;

            unwrap!(clken.set_low().ok());
        }

        self.frontend
    }
}
