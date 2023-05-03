use ads129x::{Ads129x, Error, Sample};
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

    pub async fn enable(&mut self) -> PoweredFrontend<'_, S, DRDY, RESET, TOUCH> {
        self.reset.set_high().unwrap();
        // TODO wait and configure

        PoweredFrontend {
            frontend: self,
            touched: true,
        }
    }

    pub fn is_touched(&self) -> bool {
        self.touch.is_low().unwrap()
    }

    pub fn split(self) -> (S, DRDY, RESET, TOUCH) {
        (self.adc.into_inner(), self.drdy, self.reset, self.touch)
    }
}

pub struct PoweredFrontend<'a, S, DRDY, RESET, TOUCH>
where
    RESET: OutputPin,
{
    frontend: &'a mut Frontend<S, DRDY, RESET, TOUCH>,
    touched: bool,
}

impl<'a, S, DRDY, RESET, TOUCH> PoweredFrontend<'a, S, DRDY, RESET, TOUCH>
where
    RESET: OutputPin,
    DRDY: InputPin + Wait,
    S: AsyncSpiDevice,
{
    pub async fn read(&mut self) -> Result<Sample, Error<S::Error>> {
        self.frontend.drdy.wait_for_high().await.unwrap();
        let sample = self.frontend.adc.read_data_1ch_async().await?;

        self.touched = sample.ch1_leads_connected();

        Ok(sample)
    }

    pub fn is_touched(&self) -> bool {
        self.touched
    }

    pub fn shut_down(self) {
        // Implemented in Drop
    }
}

impl<'a, S, DRDY, RESET, TOUCH> Drop for PoweredFrontend<'a, S, DRDY, RESET, TOUCH>
where
    RESET: OutputPin,
{
    fn drop(&mut self) {
        self.frontend.reset.set_low().unwrap();
    }
}
