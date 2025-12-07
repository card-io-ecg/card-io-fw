use device_driver::{AsyncCommandInterface, AsyncRegisterInterface};
use embedded_hal_async::i2c::{I2c as AsyncI2c, Operation};

device_driver::create_device!(manifest: "src/max17055.kdl");

pub struct Max17055I2cInterface<I> {
    pub i2c: I,
}

impl<I> Max17055I2cInterface<I> {
    const DEVICE_ADDR: u8 = 0x36;
}

impl<I> AsyncCommandInterface for Max17055I2cInterface<I>
where
    I: AsyncI2c,
{
    type AddressType = u16;
    type Error = I::Error;

    async fn dispatch_command(
        &mut self,
        address: Self::AddressType,
        _size_bits_in: u32,
        _input: &[u8],
        _size_bits_out: u32,
        _output: &mut [u8],
    ) -> Result<(), Self::Error> {
        self.write_register(0x60, 16, &address.to_le_bytes()).await
    }
}

impl<I> AsyncRegisterInterface for Max17055I2cInterface<I>
where
    I: AsyncI2c,
{
    type AddressType = u8;
    type Error = I::Error;

    async fn write_register(
        &mut self,
        address: Self::AddressType,
        _size_bits: u32,
        data: &[u8],
    ) -> Result<(), Self::Error> {
        self.i2c
            .transaction(
                Self::DEVICE_ADDR,
                &mut [Operation::Write(&[address]), Operation::Write(data)],
            )
            .await
    }

    async fn read_register(
        &mut self,
        address: Self::AddressType,
        _size_bits: u32,
        data: &mut [u8],
    ) -> Result<(), Self::Error> {
        self.i2c
            .transaction(
                Self::DEVICE_ADDR,
                &mut [Operation::Write(&[address]), Operation::Read(data)],
            )
            .await
    }
}
