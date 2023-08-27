use core::convert::Infallible;

use embedded_hal::digital::{ErrorType as DigitalErrorType, OutputPin};

pub struct DummyOutputPin;
impl DigitalErrorType for DummyOutputPin {
    type Error = Infallible;
}
impl OutputPin for DummyOutputPin {
    fn set_low(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn set_high(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}
