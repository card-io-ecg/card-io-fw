use core::{convert::Infallible, future::Future};

use embassy_time::Delay;
use embedded_hal::{
    digital::{ErrorType as DigitalErrorType, OutputPin},
    spi::{ErrorType as SpiErrorType, Operation},
};
use embedded_hal_async::{
    delay::DelayUs,
    spi::{SpiBus, SpiDevice},
};

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

/// A compatibility wrapper that takes ownership over SPI and turns it into an SpiDevice.
pub struct SpiDeviceWrapper<SPI, CS> {
    pub spi: SPI,
    cs: CS,
}

impl<SPI, CS> SpiErrorType for SpiDeviceWrapper<SPI, CS>
where
    SPI: SpiBus,
    CS: OutputPin,
{
    type Error = SPI::Error;
}

impl<SPI, CS> SpiDeviceWrapper<SPI, CS>
where
    SPI: SpiBus,
    CS: OutputPin,
{
    pub fn new(spi: SPI, cs: CS) -> Self {
        Self { spi, cs }
    }

    async fn lock_device<'a, F, R>(
        &'a mut self,
        op: impl FnOnce(&'a mut SPI) -> F,
    ) -> Result<R, SPI::Error>
    where
        F: Future<Output = Result<R, SPI::Error>> + 'a,
    {
        self.cs.set_low().unwrap();

        let result = op(&mut self.spi).await;

        self.cs.set_high().unwrap();

        result
    }
}

impl<SPI, CS> SpiDevice for SpiDeviceWrapper<SPI, CS>
where
    SPI: SpiBus,
    CS: OutputPin,
{
    async fn transaction(
        &mut self,
        operations: &mut [Operation<'_, u8>],
    ) -> Result<(), Self::Error> {
        self.lock_device(|spi| async {
            for op in operations {
                let res = match op {
                    Operation::Read(buf) => spi.read(buf).await,
                    Operation::Write(buf) => spi.write(buf).await,
                    Operation::Transfer(r, w) => spi.transfer(r, w).await,
                    Operation::TransferInPlace(buf) => spi.transfer_in_place(buf).await,
                    Operation::DelayUs(us) => match spi.flush().await {
                        Err(e) => Err(e),
                        Ok(()) => {
                            Delay.delay_us(*us).await;
                            Ok(())
                        }
                    },
                };

                if let Err(e) = res {
                    return Err(e);
                }
            }

            spi.flush().await
        })
        .await
    }
}
