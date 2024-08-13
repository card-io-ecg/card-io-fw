use crate::board::{
    drivers::{
        battery_monitor::battery_fg::BatteryFg as BatteryFgType,
        display::Display as DisplayType,
        frontend::{Frontend, PoweredFrontend},
    },
    utils::DummyOutputPin,
    wifi::WifiDriver,
};
use embassy_time::Delay;
use embedded_hal_bus::spi::ExclusiveDevice;
use esp_hal::{
    clock::ClockControl,
    dma::*,
    gpio::{GpioPin, Input, Io, Output},
    i2c::I2C,
    peripherals::{self, Peripherals},
    rtc_cntl::Rtc,
    spi::{master::dma::SpiDma, FullDuplexMode},
    system::SystemControl,
    timer::{ErasedTimer, OneShotTimer, PeriodicTimer},
    Async,
};

use display_interface_spi::SPIInterface;

pub type DisplaySpiInstance = peripherals::SPI2;
pub type DisplayDmaChannel = ChannelCreator0;
pub type DisplayDataCommand = GpioPin<13>;
pub type DisplayChipSelect = GpioPin<11>;
pub type DisplayReset = GpioPin<12>;
pub type DisplaySclk = GpioPin<14>;
pub type DisplayMosi = GpioPin<21>;

pub type DisplayResetPin = Output<'static, DisplayReset>;
pub type DisplayDataCommandPin = Output<'static, DisplayDataCommand>;

pub type DisplayInterface<'a> = SPIInterface<DisplaySpi<'a>, DisplayDataCommandPin>;
pub type DisplaySpi<'d> = ExclusiveDevice<
    SpiDma<'d, DisplaySpiInstance, DmaChannel0, FullDuplexMode, Async>,
    DummyOutputPin,
    Delay,
>;

pub type AdcDmaChannel = ChannelCreator1;
pub type AdcSpiInstance = peripherals::SPI3;
pub type AdcSclk = GpioPin<6>;
pub type AdcMosi = GpioPin<7>;
pub type AdcMiso = GpioPin<5>;
pub type AdcChipSelect = GpioPin<18>;
pub type AdcClockEnable = GpioPin<38>;
pub type AdcDrdy = GpioPin<4>;
pub type AdcReset = GpioPin<2>;
pub type TouchDetect = GpioPin<1>;

pub type AdcClockEnablePin = Output<'static, AdcClockEnable>;
pub type AdcDrdyPin = Input<'static, AdcDrdy>;
pub type AdcResetPin = Output<'static, AdcReset>;
pub type TouchDetectPin = Input<'static, TouchDetect>;
pub type AdcChipSelectPin = Output<'static, AdcChipSelect>;

pub type AdcSpi = ExclusiveDevice<
    SpiDma<'static, AdcSpiInstance, DmaChannel1, FullDuplexMode, Async>,
    AdcChipSelectPin,
    Delay,
>;

pub type BatteryAdcEnable = GpioPin<8>;
pub type VbusDetect = GpioPin<17>;
pub type ChargerStatus = GpioPin<47>;

pub type BatteryAdcEnablePin = Output<'static, BatteryAdcEnable>;
pub type VbusDetectPin = Input<'static, VbusDetect>;
pub type ChargerStatusPin = Input<'static, ChargerStatus>;

pub type EcgFrontend = Frontend<AdcSpi, AdcDrdyPin, AdcResetPin, AdcClockEnablePin, TouchDetectPin>;
pub type PoweredEcgFrontend =
    PoweredFrontend<AdcSpi, AdcDrdyPin, AdcResetPin, AdcClockEnablePin, TouchDetectPin>;

pub type Display = DisplayType<DisplayResetPin>;

pub type BatteryFgI2cInstance = peripherals::I2C0;
pub type I2cSda = GpioPin<36>;
pub type I2cScl = GpioPin<35>;
pub type BatteryFgI2c = I2C<'static, BatteryFgI2cInstance, Async>;
pub type BatteryFg = BatteryFgType<BatteryFgI2c, BatteryAdcEnablePin>;

impl super::startup::StartupResources {
    pub async fn initialize() -> Self {
        Self::common_init();

        let peripherals = Peripherals::take();

        let system = SystemControl::new(peripherals.SYSTEM);
        let clocks = ClockControl::max(system.clock_control).freeze();

        let systimer = esp_hal::timer::systimer::SystemTimer::new(peripherals.SYSTIMER);
        let timer = static_cell::make_static!(OneShotTimer::<ErasedTimer>::new(ErasedTimer::from(
            systimer.alarm0
        )));
        esp_hal_embassy::init(&clocks, core::slice::from_mut(timer));

        let io = Io::new(peripherals.GPIO, peripherals.IO_MUX);

        let dma = Dma::new(peripherals.DMA);

        let display = Self::create_display_driver(
            dma.channel0,
            peripherals.SPI2,
            io.pins.gpio12,
            io.pins.gpio13,
            io.pins.gpio11,
            io.pins.gpio14,
            io.pins.gpio21,
            &clocks,
        );

        let adc = Self::create_frontend_driver(
            Self::create_frontend_spi(
                dma.channel1,
                peripherals.SPI3,
                io.pins.gpio6,
                io.pins.gpio7,
                io.pins.gpio5,
                io.pins.gpio18,
                &clocks,
            ),
            io.pins.gpio4,
            io.pins.gpio2,
            io.pins.gpio38,
            io.pins.gpio1,
        );

        let battery_monitor = Self::setup_battery_monitor_fg(
            peripherals.I2C0,
            io.pins.gpio36,
            io.pins.gpio35,
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
            wifi: static_cell::make_static! {
                WifiDriver::new(
                    peripherals.WIFI,
                    PeriodicTimer::new(
                        ErasedTimer::from(esp_hal::timer::timg::TimerGroup::new(peripherals.TIMG0, &clocks)
                            .timer0)
                    ),
                    peripherals.RNG,
                    peripherals.RADIO_CLK,
                )
            },
            clocks,
            rtc: Rtc::new(peripherals.LPWR),
            software_interrupt1: system.software_interrupt_control.software_interrupt2,
        }
    }
}
