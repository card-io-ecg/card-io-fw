use ads129x::{descriptors::*, *};
use device_descriptor::Register;
use embassy_time::{Delay, Duration, Timer};
use embedded_hal::digital::{InputPin, OutputPin};
use embedded_hal_async::{digital::Wait, spi::SpiDevice as AsyncSpiDevice};
use register_access::AsyncRegisterAccess;

pub struct Frontend<S, DRDY, RESET, CLKEN, TOUCH> {
    adc: Ads129x<S>,
    drdy: DRDY,
    reset: RESET,
    clken: CLKEN,
    touch: TOUCH,
    device_id: Option<DeviceId>,
}

impl<S, DRDY, RESET, CLKEN, TOUCH> Frontend<S, DRDY, RESET, CLKEN, TOUCH> {
    pub const fn new(spi: S, drdy: DRDY, reset: RESET, clken: CLKEN, touch: TOUCH) -> Self {
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

    pub fn device_id(&self) -> Option<DeviceId> {
        self.device_id
    }
}

impl<S, DRDY, RESET, CLKEN, TOUCH> Frontend<S, DRDY, RESET, CLKEN, TOUCH>
where
    DRDY: InputPin,
    TOUCH: InputPin,
    RESET: OutputPin,
    CLKEN: OutputPin,
{
    #[rustfmt::skip]
    fn config(&self) -> ConfigRegisters {
        ConfigRegisters {
            config1: Config1::new(|r| {
                r
                .data_rate().write(DataRate::_1ksps)
                .sampling().write(Sampling::Continuous)
            }),

            config2: Config2::new(|r| {
                r
                .pdb_loff_comp().write(Buffer::Enabled)
                .ref_voltage().write(ReferenceVoltage::_2_42V)
                .clock_pin().write(ClockPin::Disabled)
                .test_signal().write(TestSignal::Disabled)
            }),

            loff: Loff::new(|r| {
                r
                .comp_th().write(ComparatorThreshold::_95)
                .leadoff_current().write(LeadOffCurrent::_22nA)
                .leadoff_frequency().write(LeadOffFrequency::DC)
            }),

            ch1set: Ch1Set::new(|r| {
                r
                .enabled().write(Channel::Enabled)
                .gain().write(Gain::X1)
                .mux().write(Ch1Mux::Normal)
            }),

            ch2set: Ch2Set::new(|r| {
                r
                .enabled().write(Channel::PowerDown)
                .gain().write(Gain::X1)
                .mux().write(Ch2Mux::Shorted)
            }),

            rldsens: RldSens::new(|r| {
                r
                .chop().write(ChopFrequency::Fmod2)
                .pdb_rld().write(Buffer::PowerDown)
                .loff_sense().write(Input::NotConnected)
                .rld2n().write(Input::NotConnected)
                .rld2p().write(Input::NotConnected)
                .rld1n().write(Input::NotConnected)
                .rld1p().write(Input::NotConnected)
            }),

            loffsens: LoffSens::new(|r| {
                r
                .flip2().write(CurrentDirection::Normal)
                .flip1().write(CurrentDirection::Normal)
                .loff2n().write(Input::NotConnected)
                .loff2p().write(Input::NotConnected)
                .loff1n().write(Input::Connected)
                .loff1p().write(Input::Connected)
            }),

            loffstat: LoffStat::new(|r| r.clk_div().write(ClockDivider::External512kHz)),

            resp1: Resp1::default(),
            resp2: Resp2::new(|r| r.rld_reference().write(RldReference::MidSupply)),

            gpio: Gpio::new(|r| {
                r
                .c2().write(PinDirection::Input)
                .c1().write(PinDirection::Output)
                .d1().write(PinState::High) // disable touch detector circuitry
            }),
        }
    }

    pub async fn enable_async(
        self,
    ) -> Result<PoweredFrontend<S, DRDY, RESET, CLKEN, TOUCH>, (Self, Error<S::Error>)>
    where
        S: AsyncSpiDevice,
    {
        let mut frontend = PoweredFrontend {
            frontend: self,
            touched: true,
        };

        match frontend.enable().await {
            Ok(_) => Ok(frontend),
            Err(err) => Err((frontend.shut_down().await, err)),
        }
    }

    pub fn is_touched(&self) -> bool {
        self.touch.is_low().unwrap()
    }

    pub async fn wait_for_release(&mut self)
    where
        TOUCH: Wait,
    {
        self.touch.wait_for_high().await.unwrap();
    }

    pub fn split(self) -> (S, DRDY, RESET, TOUCH) {
        (self.adc.into_inner(), self.drdy, self.reset, self.touch)
    }
}

pub struct PoweredFrontend<S, DRDY, RESET, CLKEN, TOUCH>
where
    RESET: OutputPin,
    CLKEN: OutputPin,
{
    frontend: Frontend<S, DRDY, RESET, CLKEN, TOUCH>,
    touched: bool,
}

impl<S, DRDY, RESET, CLKEN, TOUCH> PoweredFrontend<S, DRDY, RESET, CLKEN, TOUCH>
where
    DRDY: InputPin,
    TOUCH: InputPin,
    RESET: OutputPin,
    CLKEN: OutputPin,
{
    pub fn spi_mut(&mut self) -> &mut S {
        self.frontend.spi_mut()
    }
}

impl<S, DRDY, RESET, CLKEN, TOUCH> PoweredFrontend<S, DRDY, RESET, CLKEN, TOUCH>
where
    RESET: OutputPin,
    CLKEN: OutputPin,
    DRDY: InputPin,
    TOUCH: InputPin,
    S: AsyncSpiDevice,
{
    async fn enable(&mut self) -> Result<(), Error<S::Error>> {
        self.frontend.clken.set_high().unwrap();

        Timer::after(Duration::from_millis(1)).await;

        self.frontend
            .adc
            .reset_async(&mut self.frontend.reset, &mut Delay)
            .await?;

        let device_id = self.frontend.adc.read_device_id_async().await?;

        self.frontend.device_id = Some(device_id);

        log::info!("ADC device id: {:?}", device_id);

        let config = self.frontend.config();
        self.frontend.adc.apply_configuration_async(&config).await?;

        self.frontend
            .adc
            .write_command_async(Command::START, &mut [])
            .await?;

        Ok(())
    }

    #[allow(unused)]
    pub async fn enable_rdatac(&mut self) -> Result<(), Error<S::Error>> {
        self.frontend
            .adc
            .write_command_async(Command::RDATAC, &mut [])
            .await
    }

    pub async fn read_clksel(&mut self) -> Result<PinState, Error<S::Error>> {
        let register = self.frontend.adc.read_register_async::<Gpio>().await?;
        Ok(register.d2().read().unwrap())
    }

    pub async fn enable_fast_clock(&mut self) -> Result<(), Error<S::Error>> {
        self.frontend
            .adc
            .write_register_async::<LoffStat>(LoffStat::new(|r| {
                r.clk_div().write(ClockDivider::External2MHz)
            }))
            .await
    }

    pub async fn read(&mut self) -> Result<AdsData, Error<S::Error>>
    where
        DRDY: Wait,
    {
        self.frontend.drdy.wait_for_falling_edge().await.unwrap();

        let sample = self.frontend.adc.read_data_1ch_async().await?;
        self.touched = sample.ch1_leads_connected();

        Ok(sample)
    }

    pub fn is_touched(&self) -> bool {
        self.touched
    }

    pub async fn shut_down(mut self) -> Frontend<S, DRDY, RESET, CLKEN, TOUCH> {
        let _ = self
            .frontend
            .adc
            .write_command_async(Command::STOP, &mut [])
            .await;

        let _ = self
            .frontend
            .adc
            .write_command_async(Command::RESET, &mut [])
            .await;

        self.frontend.reset.set_low().unwrap();

        // Datasheet says to wait 2^10 clock cycles to enter power down mode. We give it a bit of
        // extra time.
        Timer::after(Duration::from_millis(5)).await;

        self.frontend.clken.set_low().unwrap();

        self.frontend
    }
}
