use core::convert::Infallible;

use embedded_hal::digital::{ErrorType as DigitalErrorType, OutputPin};
use embedded_hal_async::delay::DelayNs;

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

/// Waits in a busy loop. An `embassy_time` wait costs a timer interrupt and a task wake-up, tens
/// of microseconds, which is far longer than the nanosecond waits of the ADC protocol.
pub struct BlockingDelay;
impl DelayNs for BlockingDelay {
    async fn delay_ns(&mut self, ns: u32) {
        esp_hal::delay::Delay::new().delay_nanos(ns);
    }
}
