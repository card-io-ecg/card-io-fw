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
        Bank0GpioRegisterAccess, Bank1GpioRegisterAccess, Floating, GpioPin, Input,
        InputOutputAnalogPinType, InputOutputPinType, Output, PushPull,
        SingleCoreInteruptStatusRegisterAccessBank0, SingleCoreInteruptStatusRegisterAccessBank1,
        Unknown,
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
pub type AdcSpi<'d> = SpiDeviceWrapper<
    SpiDma<
        'd,
        hal::peripherals::SPI3,
        ChannelTx<'d, Channel1TxImpl, Channel1>,
        ChannelRx<'d, Channel1RxImpl, Channel1>,
        SuitablePeripheral1,
        FullDuplexMode,
    >,
>;

pub type BatteryAdcInput = GpioPin<
    Unknown,
    Bank0GpioRegisterAccess,
    SingleCoreInteruptStatusRegisterAccessBank0,
    InputOutputAnalogPinType,
    Gpio17Signals,
    17,
>;
pub type BatteryAdcEnable = GpioPin<
    Unknown,
    Bank0GpioRegisterAccess,
    SingleCoreInteruptStatusRegisterAccessBank0,
    InputOutputAnalogPinType,
    Gpio8Signals,
    8,
>;
pub type VbusDetect = GpioPin<
    Unknown,
    Bank1GpioRegisterAccess,
    SingleCoreInteruptStatusRegisterAccessBank1,
    InputOutputPinType,
    Gpio33Signals,
    33,
>;
pub type ChargeCurrentInput = GpioPin<
    Unknown,
    Bank0GpioRegisterAccess,
    SingleCoreInteruptStatusRegisterAccessBank0,
    InputOutputAnalogPinType,
    Gpio14Signals,
    14,
>;
pub type ChargerStatus = GpioPin<
    Unknown,
    Bank0GpioRegisterAccess,
    SingleCoreInteruptStatusRegisterAccessBank0,
    InputOutputAnalogPinType,
    Gpio21Signals,
    21,
>;

pub struct MiscPins {
    pub batt_adc_in: BatteryAdcInput,
    pub batt_adc_en: BatteryAdcEnable,
    pub vbus_detect: VbusDetect,
    pub chg_current: ChargeCurrentInput,
    pub chg_status: ChargerStatus,
}
