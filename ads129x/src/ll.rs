use device_driver::{AsyncCommandInterface, AsyncRegisterInterface};
use embedded_hal_async::spi::{Operation, SpiDevice as AsyncSpiDevice};

device_driver::create_device!(manifest: "src/ads129x.kdl");

pub(crate) const RREG: u8 = 0x20;
pub(crate) const WREG: u8 = 0x40;
// TODO: Technically, this should be config-dependent, 4 SCLK cycles.
pub(crate) const WAIT_TIME_AFTER_TRANSFER: u32 = 50;

pub struct Ads129xSpiInterface<S> {
    pub spi: S,
}

macro_rules! ops {
    (rdatac, $bytes:expr) => {
        [
            Operation::TransferInPlace($bytes),
            Operation::DelayNs(crate::ll::WAIT_TIME_AFTER_TRANSFER),
        ]
    };
    (command, $address:expr, $input:expr, $output:expr) => {
        [
            Operation::Write(&[$address]),
            Operation::Write($input),
            Operation::TransferInPlace($output),
            Operation::DelayNs(crate::ll::WAIT_TIME_AFTER_TRANSFER),
        ]
    };
    (rreg, $address:ident, $output:expr) => {
        [
            Operation::Write(&[RREG | $address, 0]),
            Operation::TransferInPlace($output),
            Operation::DelayNs(crate::ll::WAIT_TIME_AFTER_TRANSFER),
        ]
    };
    (wreg, $address:ident, $input:expr) => {
        [
            Operation::Write(&[WREG | $address, 0]),
            Operation::Write($input),
            Operation::DelayNs(crate::ll::WAIT_TIME_AFTER_TRANSFER),
        ]
    };
}

pub(crate) use ops;

impl<S> AsyncCommandInterface for Ads129xSpiInterface<S>
where
    S: AsyncSpiDevice,
{
    type Error = S::Error;
    type AddressType = u8;

    async fn dispatch_command(
        &mut self,
        address: Self::AddressType,
        _size_bits_in: u32,
        input: &[u8],
        _size_bits_out: u32,
        output: &mut [u8],
    ) -> Result<(), Self::Error> {
        self.spi
            .transaction(&mut ops!(command, address, input, output))
            .await
    }
}
impl<S> AsyncRegisterInterface for Ads129xSpiInterface<S>
where
    S: AsyncSpiDevice,
{
    type Error = S::Error;
    type AddressType = u8;

    async fn write_register(
        &mut self,
        address: Self::AddressType,
        _size_bits: u32,
        data: &[u8],
    ) -> Result<(), Self::Error> {
        self.spi.transaction(&mut ops!(wreg, address, data)).await
    }

    async fn read_register(
        &mut self,
        address: Self::AddressType,
        _size_bits: u32,
        data: &mut [u8],
    ) -> Result<(), Self::Error> {
        self.spi.transaction(&mut ops!(rreg, address, data)).await
    }
}
