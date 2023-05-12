use embedded_hal::spi::{ErrorType, Operation};
use embedded_hal_async::spi::{SpiBus, SpiDevice, SpiDeviceRead, SpiDeviceWrite};

/// A compatibility wrapper that takes ownership over SPI and turns it into an SpiDevice.
pub struct SpiDeviceWrapper<SPI> {
    pub spi: SPI,
}

impl<SPI> ErrorType for SpiDeviceWrapper<SPI>
where
    SPI: SpiBus,
{
    type Error = SPI::Error;
}

impl<SPI> SpiDeviceRead for SpiDeviceWrapper<SPI>
where
    SPI: SpiBus<u8>,
{
    async fn read_transaction(&mut self, operations: &mut [&mut [u8]]) -> Result<(), Self::Error> {
        for op in operations {
            self.spi.read(op).await?;
        }
        Ok(())
    }
}

impl<SPI> SpiDeviceWrite for SpiDeviceWrapper<SPI>
where
    SPI: SpiBus,
{
    async fn write_transaction(&mut self, operations: &[&[u8]]) -> Result<(), Self::Error> {
        for op in operations {
            self.spi.write(op).await?;
        }
        Ok(())
    }
}

impl<SPI> SpiDevice for SpiDeviceWrapper<SPI>
where
    SPI: SpiBus,
{
    async fn transaction(
        &mut self,
        operations: &mut [Operation<'_, u8>],
    ) -> Result<(), Self::Error> {
        for op in operations {
            match op {
                Operation::Read(buf) => self.spi.read(buf).await?,
                Operation::Write(buf) => self.spi.write(buf).await?,
                Operation::Transfer(r, w) => self.spi.transfer(r, w).await?,
                Operation::TransferInPlace(buf) => self.spi.transfer_in_place(buf).await?,
            }
        }

        Ok(())
    }
}
