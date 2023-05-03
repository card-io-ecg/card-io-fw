use core::future::Future;

use embedded_hal::{
    digital::OutputPin,
    spi::{ErrorType, Operation},
};
use embedded_hal_async::spi::{SpiBus, SpiDevice, SpiDeviceRead, SpiDeviceWrite};

/// A compatibility wrapper that takes ownership over SPI and turns it into an SpiDevice.
pub struct SpiDeviceWrapper<SPI, CS> {
    pub spi: SPI,
    pub chip_select: CS,
}

impl<SPI, CS> SpiDeviceWrapper<SPI, CS>
where
    SPI: SpiBus,
    CS: OutputPin,
{
    async fn selected_async<'a, R, F>(&'a mut self, op: impl FnOnce(&'a mut SPI) -> F) -> R
    where
        F: Future<Output = R> + 'a,
    {
        self.chip_select.set_low().unwrap();
        let r = op(&mut self.spi).await;
        self.chip_select.set_high().unwrap();
        r
    }
}

impl<SPI, CS> ErrorType for SpiDeviceWrapper<SPI, CS>
where
    SPI: SpiBus,
    CS: OutputPin,
{
    type Error = SPI::Error;
}

impl<SPI, CS> SpiDeviceRead for SpiDeviceWrapper<SPI, CS>
where
    SPI: SpiBus<u8>,
    CS: OutputPin,
{
    async fn read_transaction(&mut self, operations: &mut [&mut [u8]]) -> Result<(), Self::Error> {
        self.selected_async(|spi| async {
            for op in operations {
                spi.read(op).await?;
            }
            Ok(())
        })
        .await
    }
}

impl<SPI, CS> SpiDeviceWrite for SpiDeviceWrapper<SPI, CS>
where
    SPI: SpiBus,
    CS: OutputPin,
{
    async fn write_transaction(&mut self, operations: &[&[u8]]) -> Result<(), Self::Error> {
        self.selected_async(|spi| async {
            for op in operations {
                spi.write(op).await?;
            }
            Ok(())
        })
        .await
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
        self.selected_async(|spi| async {
            for op in operations {
                match op {
                    Operation::Read(buf) => spi.read(buf).await?,
                    Operation::Write(buf) => spi.write(buf).await?,
                    Operation::Transfer(r, w) => spi.transfer(r, w).await?,
                    Operation::TransferInPlace(buf) => spi.transfer_in_place(buf).await?,
                }
            }

            Ok(())
        })
        .await
    }
}
