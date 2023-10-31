#[cfg(feature = "battery_adc")]
use crate::board::{
    drivers::battery_monitor::{battery_adc::BatteryAdc as BatteryAdcType, BatteryMonitor},
    hal::{adc::ADC1, gpio::Analog},
};

#[cfg(feature = "battery_max17055")]
use crate::board::{
    drivers::battery_monitor::battery_fg::BatteryFg as BatteryFgType,
    hal::{gpio::Unknown, i2c::I2C},
};

use crate::board::{
    drivers::{
        display::Display as DisplayType,
        frontend::{Frontend, PoweredFrontend},
    },
    hal::{
        self,
        clock::{ClockControl, CpuClock},
        embassy,
        gdma::*,
        gpio::{Floating, GpioPin, Input, Output, PullUp, PushPull},
        peripherals::{self, Peripherals},
        prelude::*,
        spi::{master::dma::SpiDma, FullDuplexMode},
        systimer::SystemTimer,
        Rtc, IO,
    },
    startup::WIFI_DRIVER,
    utils::DummyOutputPin,
    wifi::WifiDriver,
};
use embassy_time::Delay;
use embedded_hal_bus::spi::ExclusiveDevice;

use display_interface_spi::SPIInterface;

pub type DisplaySpiInstance = hal::peripherals::SPI2;
pub type DisplayDmaChannel = ChannelCreator0;
pub type DisplayDataCommand = GpioPin<Output<PushPull>, 13>;
pub type DisplayChipSelect = GpioPin<Output<PushPull>, 11>;
pub type DisplayReset = GpioPin<Output<PushPull>, 12>;
pub type DisplaySclk = GpioPin<Output<PushPull>, 14>;
pub type DisplayMosi = GpioPin<Output<PushPull>, 21>;

pub type DisplayInterface<'a> = SPIInterface<DisplaySpi<'a>, DisplayDataCommand>;
pub type DisplaySpi<'d> = ExclusiveDevice<
    SpiDma<'d, DisplaySpiInstance, Channel0, FullDuplexMode>,
    DummyOutputPin,
    Delay,
>;

pub type AdcDmaChannel = ChannelCreator1;
pub type AdcSpiInstance = hal::peripherals::SPI3;
pub type AdcSclk = GpioPin<Output<PushPull>, 6>;
pub type AdcMosi = GpioPin<Output<PushPull>, 7>;
pub type AdcMiso = GpioPin<Input<Floating>, 5>;
pub type AdcChipSelect = GpioPin<Output<PushPull>, 18>;
pub type AdcClockEnable = GpioPin<Output<PushPull>, 38>;
pub type AdcDrdy = GpioPin<Input<Floating>, 4>;
pub type AdcReset = GpioPin<Output<PushPull>, 2>;
pub type TouchDetect = GpioPin<Input<Floating>, 1>;
pub type AdcSpi<'d> =
    ExclusiveDevice<SpiDma<'d, AdcSpiInstance, Channel1, FullDuplexMode>, AdcChipSelect, Delay>;

pub type VbusDetect = GpioPin<Input<Floating>, 17>;
pub type ChargerStatus = GpioPin<Input<PullUp>, 47>;

pub type EcgFrontend = Frontend<AdcSpi<'static>, AdcDrdy, AdcReset, AdcClockEnable, TouchDetect>;
pub type PoweredEcgFrontend =
    PoweredFrontend<AdcSpi<'static>, AdcDrdy, AdcReset, AdcClockEnable, TouchDetect>;

pub type Display = DisplayType<DisplayReset>;

#[cfg(feature = "battery_max17055")]
mod battery_monitor_types {
    use super::*;
    pub type BatteryFgI2cInstance = hal::peripherals::I2C0;
    pub type I2cSda = GpioPin<Unknown, 35>;
    pub type I2cScl = GpioPin<Unknown, 36>;
    pub type BatteryFgI2c = I2C<'static, BatteryFgI2cInstance>;
    pub type BatteryAdcEnable = GpioPin<Output<PushPull>, 8>;
    pub type BatteryFg = BatteryFgType<BatteryFgI2c, BatteryAdcEnable>;
}

#[cfg(feature = "battery_adc")]
mod battery_monitor_types {
    use super::*;
    pub type ChargeCurrentInput = GpioPin<Analog, 10>;
    pub type BatteryAdcInput = GpioPin<Analog, 9>;
    pub type BatteryAdcEnable = GpioPin<Output<PushPull>, 8>;

    pub type BatteryAdc =
        BatteryAdcType<BatteryAdcInput, ChargeCurrentInput, BatteryAdcEnable, ADC1>;
}

pub use battery_monitor_types::*;

impl super::startup::StartupResources {
    pub async fn initialize() -> Self {
        Self::common_init();

        let peripherals = Peripherals::take();

        let system = peripherals.SYSTEM.split();
        let clocks = ClockControl::configure(system.clock_control, CpuClock::Clock240MHz).freeze();

        embassy::init(&clocks, SystemTimer::new(peripherals.SYSTIMER));

        let io = IO::new(peripherals.GPIO, peripherals.IO_MUX);

        let dma = Gdma::new(peripherals.DMA);

        let display = Self::create_display_driver(
            dma.channel0,
            peripherals::Interrupt::DMA_IN_CH0,
            peripherals::Interrupt::DMA_OUT_CH0,
            peripherals.SPI2,
            io.pins.gpio12,
            io.pins.gpio13,
            io.pins.gpio11,
            io.pins.gpio14,
            io.pins.gpio21,
            &clocks,
        );

        let adc = Self::create_frontend_driver(
            dma.channel1,
            peripherals::Interrupt::DMA_IN_CH1,
            peripherals::Interrupt::DMA_OUT_CH1,
            peripherals.SPI3,
            io.pins.gpio6,
            io.pins.gpio7,
            io.pins.gpio5,
            io.pins.gpio4,
            io.pins.gpio2,
            io.pins.gpio38,
            io.pins.gpio1,
            io.pins.gpio18,
            &clocks,
        );

        // Battery ADC
        #[cfg(feature = "battery_adc")]
        let battery_monitor = {
            let analog = peripherals.SENS.split();
            BatteryMonitor::start(
                io.pins.gpio17.into(),
                io.pins.gpio47.into(),
                BatteryAdc::new(analog.adc1, io.pins.gpio9, io.pins.gpio10, io.pins.gpio8),
            )
            .await
        };

        #[cfg(feature = "battery_max17055")]
        let battery_monitor = Self::setup_batter_monitor_fg(
            peripherals.I2C0,
            peripherals::Interrupt::I2C_EXT0,
            io.pins.gpio35,
            io.pins.gpio36,
            io.pins.gpio17,
            io.pins.gpio47,
            io.pins.gpio8,
            &clocks,
        )
        .await;

        Self {
            display,
            frontend: adc,
            battery_monitor,
            wifi: WIFI_DRIVER.init_with(|| {
                WifiDriver::new(
                    peripherals.WIFI,
                    peripherals.TIMG1,
                    peripherals.RNG,
                    system.radio_clock_control,
                    &clocks,
                )
            }),
            clocks,
            rtc: Rtc::new(peripherals.RTC_CNTL),
        }
    }
}
