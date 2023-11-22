use crate::board::{
    drivers::{
        battery_monitor::battery_fg::BatteryFg as BatteryFgType,
        display::Display as DisplayType,
        frontend::{Frontend, PoweredFrontend},
    },
    hal::{
        self,
        clock::ClockControl,
        embassy,
        gdma::*,
        gpio::{Floating, GpioPin, Input, Output, PullUp, PushPull, Unknown},
        i2c::I2C,
        peripherals::{self, Peripherals},
        prelude::*,
        spi::{master::dma::SpiDma, FullDuplexMode},
        systimer::SystemTimer,
        timer::TimerGroup,
        Rtc, IO,
    },
    utils::DummyOutputPin,
    wifi::WifiDriver,
};
use display_interface_spi::SPIInterface;
use embassy_time::Delay;
use embedded_hal_bus::spi::ExclusiveDevice;

#[cfg(feature = "esp32s3")]
mod hw {
    use super::*;

    pub type DisplaySpiInstance = hal::peripherals::SPI2;
    pub type DisplayDmaChannel = ChannelCreator0;
    pub type DisplayDataCommand = GpioPin<Output<PushPull>, 17>;
    pub type DisplayChipSelect = GpioPin<Output<PushPull>, 8>;
    pub type DisplayReset = GpioPin<Output<PushPull>, 18>;
    pub type DisplaySclk = GpioPin<Output<PushPull>, 39>;
    pub type DisplayMosi = GpioPin<Output<PushPull>, 38>;

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
    pub type AdcChipSelect = GpioPin<Output<PushPull>, 0>;
    pub type AdcClockEnable = GpioPin<Output<PushPull>, 2>;
    pub type AdcDrdy = GpioPin<Input<Floating>, 4>;
    pub type AdcReset = GpioPin<Output<PushPull>, 42>;
    pub type TouchDetect = GpioPin<Input<Floating>, 1>;
    pub type AdcSpiBus = SpiDma<'static, AdcSpiInstance, Channel1, FullDuplexMode>;
    pub type AdcSpi = ExclusiveDevice<AdcSpiBus, AdcChipSelect, Delay>;

    pub type BatteryAdcEnable = DummyOutputPin;
    pub type VbusDetect = GpioPin<Input<Floating>, 40>;
    pub type ChargerStatus = GpioPin<Input<PullUp>, 37>;

    pub type EcgFrontend = Frontend<AdcSpi, AdcDrdy, AdcReset, AdcClockEnable, TouchDetect>;
    pub type PoweredEcgFrontend =
        PoweredFrontend<AdcSpi, AdcDrdy, AdcReset, AdcClockEnable, TouchDetect>;

    pub type Display = DisplayType<DisplayReset>;

    pub type BatteryFgI2cInstance = hal::peripherals::I2C0;
    pub type I2cSda = GpioPin<Unknown, 36>;
    pub type I2cScl = GpioPin<Unknown, 35>;
    pub type BatteryFgI2c = I2C<'static, BatteryFgI2cInstance>;
    pub type BatteryFg = BatteryFgType<BatteryFgI2c, BatteryAdcEnable>;
}

#[cfg(feature = "esp32c6")]
mod hw {
    use super::*;
    pub use crate::board::drivers::bitbang_spi::BitbangSpi;

    pub type DisplaySpiInstance = hal::peripherals::SPI2;
    pub type DisplayDmaChannel = ChannelCreator0;
    pub type DisplayDataCommand = GpioPin<Output<PushPull>, 8>;
    pub type DisplayChipSelect = GpioPin<Output<PushPull>, 11>;
    pub type DisplayReset = GpioPin<Output<PushPull>, 10>;
    pub type DisplaySclk = GpioPin<Output<PushPull>, 22>;
    pub type DisplayMosi = GpioPin<Output<PushPull>, 21>;

    pub type DisplayInterface<'a> = SPIInterface<DisplaySpi<'a>, DisplayDataCommand>;
    pub type DisplaySpi<'d> = ExclusiveDevice<
        SpiDma<'d, DisplaySpiInstance, Channel0, FullDuplexMode>,
        DummyOutputPin,
        Delay,
    >;

    pub type AdcSclk = GpioPin<Output<PushPull>, 6>;
    pub type AdcMosi = GpioPin<Output<PushPull>, 7>;
    pub type AdcMiso = GpioPin<Input<Floating>, 5>;
    pub type AdcChipSelect = GpioPin<Output<PushPull>, 9>;
    pub type AdcClockEnable = GpioPin<Output<PushPull>, 23>;
    pub type AdcDrdy = GpioPin<Input<Floating>, 4>;
    pub type AdcReset = GpioPin<Output<PushPull>, 15>;
    pub type TouchDetect = GpioPin<Input<Floating>, 2>;
    pub type AdcSpiBus = BitbangSpi<AdcMosi, AdcMiso, AdcSclk>;
    pub type AdcSpi = ExclusiveDevice<AdcSpiBus, AdcChipSelect, Delay>;

    pub type BatteryAdcEnable = DummyOutputPin;
    pub type VbusDetect = GpioPin<Input<Floating>, 3>;
    pub type ChargerStatus = GpioPin<Input<PullUp>, 20>;

    pub type EcgFrontend = Frontend<AdcSpi, AdcDrdy, AdcReset, AdcClockEnable, TouchDetect>;
    pub type PoweredEcgFrontend =
        PoweredFrontend<AdcSpi, AdcDrdy, AdcReset, AdcClockEnable, TouchDetect>;

    pub type Display = DisplayType<DisplayReset>;

    pub type BatteryFgI2cInstance = hal::peripherals::I2C0;
    pub type I2cSda = GpioPin<Unknown, 19>;
    pub type I2cScl = GpioPin<Unknown, 18>;
    pub type BatteryFgI2c = I2C<'static, BatteryFgI2cInstance>;
    pub type BatteryFg = BatteryFgType<BatteryFgI2c, BatteryAdcEnable>;
}

pub use hw::*;

impl super::startup::StartupResources {
    pub async fn initialize() -> Self {
        Self::common_init();

        let peripherals = Peripherals::take();

        let system = peripherals.SYSTEM.split();
        let clocks = ClockControl::max(system.clock_control).freeze();

        #[cfg(feature = "esp32s3")]
        embassy::init(&clocks, SystemTimer::new(peripherals.SYSTIMER));

        #[cfg(feature = "esp32c6")]
        embassy::init(&clocks, TimerGroup::new(peripherals.TIMG0, &clocks).timer0);

        let io = IO::new(peripherals.GPIO, peripherals.IO_MUX);

        let dma = Gdma::new(peripherals.DMA);

        #[cfg(feature = "esp32s3")]
        let display = Self::create_display_driver(
            dma.channel0,
            peripherals::Interrupt::DMA_IN_CH0,
            peripherals::Interrupt::DMA_OUT_CH0,
            peripherals.SPI2,
            io.pins.gpio18,
            io.pins.gpio17,
            io.pins.gpio8,
            io.pins.gpio39,
            io.pins.gpio38,
            &clocks,
        );

        #[cfg(feature = "esp32c6")]
        let display = Self::create_display_driver(
            dma.channel0,
            peripherals::Interrupt::DMA_IN_CH0,
            peripherals::Interrupt::DMA_OUT_CH0,
            peripherals.SPI2,
            io.pins.gpio10,
            io.pins.gpio8,
            io.pins.gpio11,
            io.pins.gpio22,
            io.pins.gpio21,
            &clocks,
        );

        #[cfg(feature = "esp32s3")]
        let adc = Self::create_frontend_driver(
            Self::create_frontend_spi(
                dma.channel1,
                peripherals::Interrupt::DMA_IN_CH1,
                peripherals::Interrupt::DMA_OUT_CH1,
                peripherals.SPI3,
                io.pins.gpio6,
                io.pins.gpio7,
                io.pins.gpio5,
                io.pins.gpio0,
                &clocks,
            ),
            io.pins.gpio4,
            io.pins.gpio42,
            io.pins.gpio2,
            io.pins.gpio1,
        );

        #[cfg(feature = "esp32c6")]
        let adc = Self::create_frontend_driver(
            ExclusiveDevice::new(
                BitbangSpi::new(
                    io.pins.gpio7.into(),
                    io.pins.gpio5.into(),
                    io.pins.gpio6.into(),
                    1u32.MHz(),
                ),
                io.pins.gpio9.into(),
                Delay,
            ),
            io.pins.gpio4,
            io.pins.gpio15,
            io.pins.gpio23,
            io.pins.gpio2,
        );

        #[cfg(feature = "esp32s3")]
        let battery_monitor = Self::setup_battery_monitor_fg(
            peripherals.I2C0,
            peripherals::Interrupt::I2C_EXT0,
            io.pins.gpio36,
            io.pins.gpio35,
            io.pins.gpio40,
            io.pins.gpio37,
            DummyOutputPin,
            &clocks,
        )
        .await;

        #[cfg(feature = "esp32c6")]
        let battery_monitor = Self::setup_battery_monitor_fg(
            peripherals.I2C0,
            peripherals::Interrupt::I2C_EXT0,
            io.pins.gpio19,
            io.pins.gpio18,
            io.pins.gpio3,
            io.pins.gpio20,
            DummyOutputPin,
            &clocks,
        )
        .await;

        Self {
            display,
            frontend: adc,
            battery_monitor,
            wifi: static_cell::make_static! {
                WifiDriver::new(
                    peripherals.WIFI,
                    #[cfg(feature = "esp32s3")]
                    TimerGroup::new(peripherals.TIMG1, &clocks).timer0,
                    #[cfg(feature = "esp32c6")]
                    SystemTimer::new(peripherals.SYSTIMER).alarm0,
                    peripherals.RNG,
                    system.radio_clock_control,
                )
            },
            clocks,
            #[cfg(feature = "esp32s3")]
            rtc: Rtc::new(peripherals.RTC_CNTL),
            #[cfg(feature = "esp32c6")]
            rtc: Rtc::new(peripherals.LP_CLKRST),
        }
    }
}
