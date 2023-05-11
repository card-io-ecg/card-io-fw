use ads129x::{descriptors::*, *};
use device_descriptor::Register;
use embassy_time::Delay;
use embedded_hal::digital::{InputPin, OutputPin};
use embedded_hal_async::{digital::Wait, spi::SpiDevice as AsyncSpiDevice};

pub struct Frontend<S, DRDY, RESET, TOUCH> {
    adc: Ads129x<S>,
    drdy: DRDY,
    reset: RESET,
    touch: TOUCH,
}

impl<S, DRDY, RESET, TOUCH> Frontend<S, DRDY, RESET, TOUCH>
where
    DRDY: InputPin,
    TOUCH: InputPin,
    RESET: OutputPin,
{
    pub const fn new(spi: S, drdy: DRDY, reset: RESET, touch: TOUCH) -> Self {
        Self {
            adc: Ads129x::new(spi),
            drdy,
            reset,
            touch,
        }
    }

    fn config(&self) -> ConfigRegisters {
        ConfigRegisters {
            config1: Config1::new(|r| {
                r.data_rate()
                    .write(DataRate::_1ksps)
                    .sampling()
                    .write(Sampling::Continuous)
            }),

            config2: Config2::new(|r| {
                r.pdb_loff_comp()
                    .write(Buffer::Enabled)
                    .ref_voltage()
                    .write(ReferenceVoltage::_2_42V)
                    .clock_pin()
                    .write(ClockPin::Disabled)
                    .test_signal()
                    .write(TestSignal::Disabled)
            }),

            loff: Loff::new(|r| {
                r.comp_th()
                    .write(ComparatorThreshold::_95)
                    .leadoff_current()
                    .write(LeadOffCurrent::_6nA)
                    .leadoff_frequency()
                    .write(LeadOffFrequency::DC)
            }),

            ch1set: Ch1Set::new(|r| {
                r.enabled()
                    .write(Channel::Enabled)
                    .gain()
                    .write(Gain::x6)
                    .mux()
                    .write(Ch1Mux::Normal)
            }),

            ch2set: Ch2Set::new(|r| {
                r.enabled()
                    .write(Channel::PowerDown)
                    .gain()
                    .write(Gain::x1)
                    .mux()
                    .write(Ch2Mux::Shorted)
            }),

            rldsens: RldSens::new(|r| {
                r.chop()
                    .write(ChopFrequency::Fmod2)
                    .pdb_rld()
                    .write(Buffer::Enabled)
                    .loff_sense()
                    .write(Input::NotConnected)
                    .rld2n()
                    .write(Input::NotConnected)
                    .rld2p()
                    .write(Input::NotConnected)
                    .rld1n()
                    .write(Input::Connected)
                    .rld1p()
                    .write(Input::Connected)
            }),

            loffsens: LoffSens::new(|r| {
                r.flip2()
                    .write(CurrentDirection::Normal)
                    .flip1()
                    .write(CurrentDirection::Normal)
                    .loff2n()
                    .write(Input::NotConnected)
                    .loff2p()
                    .write(Input::NotConnected)
                    .loff1n()
                    .write(Input::Connected)
                    .loff1p()
                    .write(Input::NotConnected)
            }),

            loffstat: LoffStat::new(|r| {
                // TODO support internal 512kHz
                r.clk_div().write(ClockDivider::External2MHz)
            }),
            resp1: Resp1::default(),

            resp2: Resp2::new(|r| r.rld_reference().write(RldReference::MidSupply)),

            gpio: Gpio::new(|r| {
                r.c2()
                    .write(PinDirection::Input)
                    .c1()
                    .write(PinDirection::Output)
                    .d1()
                    .write(PinState::High) // disable touch detector circuitry
            }),
        }
    }

    pub fn spi_mut(&mut self) -> &mut S {
        self.adc.inner_mut()
    }

    pub async fn enable_async(
        mut self,
    ) -> Result<PoweredFrontend<S, DRDY, RESET, TOUCH>, Error<S::Error>>
    where
        S: AsyncSpiDevice,
    {
        self.adc.reset_async(&mut self.reset, &mut Delay).await;

        let config = self.config();

        let device_id = self.adc.read_device_id_async().await?;
        log::info!("ADC device id: {:?}", device_id);

        self.adc.apply_configuration_async(&config).await?;

        Ok(PoweredFrontend {
            frontend: self,
            touched: true,
        })
    }

    pub fn is_touched(&self) -> bool {
        self.touch.is_low().unwrap()
    }

    pub async fn wait_for_touch(&mut self)
    where
        TOUCH: Wait,
    {
        self.touch.wait_for_low().await;
    }

    pub fn split(self) -> (S, DRDY, RESET, TOUCH) {
        (self.adc.into_inner(), self.drdy, self.reset, self.touch)
    }
}

pub struct PoweredFrontend<S, DRDY, RESET, TOUCH>
where
    RESET: OutputPin,
{
    frontend: Frontend<S, DRDY, RESET, TOUCH>,
    touched: bool,
}

impl<S, DRDY, RESET, TOUCH> PoweredFrontend<S, DRDY, RESET, TOUCH>
where
    DRDY: InputPin,
    TOUCH: InputPin,
    RESET: OutputPin,
{
    pub fn spi_mut(&mut self) -> &mut S {
        self.frontend.spi_mut()
    }
}

impl<S, DRDY, RESET, TOUCH> PoweredFrontend<S, DRDY, RESET, TOUCH>
where
    RESET: OutputPin,
    DRDY: InputPin + Wait,
    S: AsyncSpiDevice,
{
    pub async fn read(&mut self) -> Result<AdsData, Error<S::Error>> {
        self.frontend.drdy.wait_for_high().await.unwrap();
        let sample = self.frontend.adc.read_data_1ch_async().await?;

        self.touched = sample.ch1_leads_connected();

        Ok(sample)
    }

    pub fn is_touched(&self) -> bool {
        self.touched
    }

    pub fn shut_down(mut self) -> Frontend<S, DRDY, RESET, TOUCH> {
        self.frontend.reset.set_low().unwrap();
        self.frontend
    }
}
