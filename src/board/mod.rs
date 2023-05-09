use esp_backtrace as _;

#[cfg(feature = "esp32s2")]
pub use esp32s2_hal as hal;

#[cfg(feature = "esp32s3")]
pub use esp32s3_hal as hal;

#[cfg(feature = "esp32s2")]
pub use esp32s2 as pac;

#[cfg(feature = "esp32s3")]
pub use esp32s3 as pac;

use crate::spi_device::SpiDeviceWrapper;
use display_interface_spi_async::SPIInterface;
use hal::{
    dma::{ChannelRx, ChannelTx},
    gdma::*,
    gpio::{
        Bank0GpioRegisterAccess, Floating, GpioPin, Input, InputOutputAnalogPinType, Output,
        PushPull, SingleCoreInteruptStatusRegisterAccessBank0,
    },
    soc::gpio::*,
    spi::{dma::SpiDma, FullDuplexMode},
};

pub mod initialized;
pub mod startup;

pub type DisplaySpi<'d> = SpiDma<
    'd,
    hal::peripherals::SPI2,
    ChannelTx<'d, Channel0TxImpl, Channel0>,
    ChannelRx<'d, Channel0RxImpl, Channel0>,
    SuitablePeripheral0,
    FullDuplexMode,
>;

pub type DisplayDataCommand = GpioPin<
    Output<PushPull>,
    Bank0GpioRegisterAccess,
    SingleCoreInteruptStatusRegisterAccessBank0,
    InputOutputAnalogPinType,
    Gpio13Signals,
    13,
>;
pub type DisplayChipSelect = GpioPin<
    Output<PushPull>,
    Bank0GpioRegisterAccess,
    SingleCoreInteruptStatusRegisterAccessBank0,
    InputOutputAnalogPinType,
    Gpio10Signals,
    10,
>;
pub type DisplayReset = GpioPin<
    Output<PushPull>,
    Bank0GpioRegisterAccess,
    SingleCoreInteruptStatusRegisterAccessBank0,
    InputOutputAnalogPinType,
    Gpio9Signals,
    9,
>;

pub type DisplayInterface<'a> = SPIInterface<DisplaySpi<'a>, DisplayDataCommand, DisplayChipSelect>;

pub type AdcDrdy = GpioPin<
    Input<Floating>,
    Bank0GpioRegisterAccess,
    SingleCoreInteruptStatusRegisterAccessBank0,
    InputOutputAnalogPinType,
    Gpio4Signals,
    4,
>;
pub type AdcReset = GpioPin<
    Output<PushPull>,
    Bank0GpioRegisterAccess,
    SingleCoreInteruptStatusRegisterAccessBank0,
    InputOutputAnalogPinType,
    Gpio2Signals,
    2,
>;
pub type TouchDetect = GpioPin<
    Input<Floating>,
    Bank0GpioRegisterAccess,
    SingleCoreInteruptStatusRegisterAccessBank0,
    InputOutputAnalogPinType,
    Gpio1Signals,
    1,
>;
pub type AdcChipSelect = GpioPin<
    Output<PushPull>,
    Bank0GpioRegisterAccess,
    SingleCoreInteruptStatusRegisterAccessBank0,
    InputOutputAnalogPinType,
    Gpio18Signals,
    18,
>;
pub type AdcSpi<'d> = SpiDeviceWrapper<
    SpiDma<
        'd,
        hal::peripherals::SPI3,
        ChannelTx<'d, Channel1TxImpl, Channel1>,
        ChannelRx<'d, Channel1RxImpl, Channel1>,
        SuitablePeripheral1,
        FullDuplexMode,
    >,
    AdcChipSelect,
>;
