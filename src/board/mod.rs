pub mod drivers;
pub mod initialized;
pub mod startup;
pub mod utils;
pub mod wifi_driver;

use esp_backtrace as _;

#[cfg(feature = "esp32s2")]
pub use esp32s2_hal as hal;

#[cfg(feature = "esp32s3")]
pub use esp32s3_hal as hal;

#[cfg(feature = "esp32s2")]
pub use esp32s2 as pac;

#[cfg(feature = "esp32s3")]
pub use esp32s3 as pac;
use gui::{
    screens::display_menu::{BatteryDisplayStyle, DisplayBrightness},
    widgets::battery_small::BatteryStyle,
};
use signal_processing::battery::BatteryModel;

use display_interface_spi::SPIInterface;
use drivers::{
    battery_adc::BatteryAdc as BatteryAdcType,
    display::{Display as DisplayType, PoweredDisplay as PoweredDisplayType},
    frontend::{Frontend, PoweredFrontend},
};
use hal::{
    adc::ADC2,
    dma::{ChannelRx, ChannelTx},
    gdma::*,
    gpio::{Analog, Floating, GpioPin, Input, Output, PullUp, PushPull},
    spi::{dma::SpiDma, FullDuplexMode},
};
use ssd1306::prelude::Brightness;
use utils::{DummyOutputPin, SpiDeviceWrapper};

pub type DisplaySpi<'d> = SpiDeviceWrapper<
    SpiDma<
        'd,
        hal::peripherals::SPI2,
        ChannelTx<'d, Channel0TxImpl, Channel0>,
        ChannelRx<'d, Channel0RxImpl, Channel0>,
        SuitablePeripheral0,
        FullDuplexMode,
    >,
    DummyOutputPin,
>;

pub type DisplayDataCommand = GpioPin<Output<PushPull>, 13>;
pub type DisplayChipSelect = GpioPin<Output<PushPull>, 10>;
pub type DisplayReset = GpioPin<Output<PushPull>, 9>;

pub type DisplayInterface<'a> = SPIInterface<DisplaySpi<'a>, DisplayDataCommand>;

pub type AdcDrdy = GpioPin<Input<Floating>, 4>;
pub type AdcReset = GpioPin<Output<PushPull>, 2>;
pub type TouchDetect = GpioPin<Input<Floating>, 1>;
pub type AdcChipSelect = GpioPin<Output<PushPull>, 18>;
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

pub type BatteryAdcInput = GpioPin<Analog, 17>;
pub type BatteryAdcEnable = GpioPin<Output<PushPull>, 8>;
pub type VbusDetect = GpioPin<Input<Floating>, 47>;
pub type ChargeCurrentInput = GpioPin<Analog, 14>;
pub type ChargerStatus = GpioPin<Input<PullUp>, 21>;

pub type EcgFrontend = Frontend<AdcSpi<'static>, AdcDrdy, AdcReset, TouchDetect>;
pub type PoweredEcgFrontend = PoweredFrontend<AdcSpi<'static>, AdcDrdy, AdcReset, TouchDetect>;

pub type Display = DisplayType<DisplayInterface<'static>, DisplayReset>;
pub type PoweredDisplay = PoweredDisplayType<DisplayInterface<'static>, DisplayReset>;

pub type BatteryAdc = BatteryAdcType<BatteryAdcInput, ChargeCurrentInput, BatteryAdcEnable, ADC2>;

pub struct MiscPins {
    pub vbus_detect: VbusDetect,
    pub chg_status: ChargerStatus,
}

pub const BATTERY_MODEL: BatteryModel = BatteryModel {
    voltage: (2750, 4200),
    charge_current: (0, 1000),
};

pub const LOW_BATTERY_VOLTAGE: u16 = 3300;

pub struct Config {
    pub battery_display_style: BatteryDisplayStyle,
    pub display_brightness: DisplayBrightness,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            battery_display_style: BatteryDisplayStyle::Indicator,
            display_brightness: DisplayBrightness::Normal,
        }
    }
}

impl Config {
    pub fn battery_style(&self) -> BatteryStyle {
        BatteryStyle::new(self.battery_display_style, BATTERY_MODEL)
    }

    pub fn display_brightness(&self) -> Brightness {
        match self.display_brightness {
            DisplayBrightness::Dimmest => Brightness::DIMMEST,
            DisplayBrightness::Dim => Brightness::DIM,
            DisplayBrightness::Normal => Brightness::NORMAL,
            DisplayBrightness::Bright => Brightness::BRIGHT,
            DisplayBrightness::Brightest => Brightness::BRIGHTEST,
        }
    }
}
