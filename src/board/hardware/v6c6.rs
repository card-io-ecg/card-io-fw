use crate::board::{
    drivers::{
        battery_monitor::battery_fg::BatteryFg as BatteryFgType,
        display::Display as DisplayType,
        frontend::{Frontend, PoweredFrontend},
    },
    utils::DummyOutputPin,
    wifi::WifiDriver,
};
use display_interface_spi::SPIInterface;
use embassy_time::Delay;
use embedded_hal_bus::spi::ExclusiveDevice;
use esp_hal::{
    dma::*,
    gpio::{GpioPin, Input, Io, Level, Output, Pull},
    i2c::I2C,
    interrupt::software::SoftwareInterruptControl,
    peripherals,
    prelude::*,
    rtc_cntl::Rtc,
    spi::{master::SpiDmaBus, FullDuplexMode},
    timer::{
        systimer::{SystemTimer, Target},
        timg::TimerGroup,
        AnyTimer,
    },
    Async,
};

pub use crate::board::drivers::bitbang_spi::BitbangSpi;

pub type DisplaySpiInstance = peripherals::SPI2;
pub type DisplayDmaChannel = ChannelCreator0;
pub type DisplayDataCommand = GpioPin<8>;
pub type DisplayChipSelect = GpioPin<11>;
pub type DisplayReset = GpioPin<10>;
pub type DisplaySclk = GpioPin<22>;
pub type DisplayMosi = GpioPin<21>;

pub type DisplayResetPin = Output<'static, DisplayReset>;
pub type DisplayDataCommandPin = Output<'static, DisplayDataCommand>;

pub type DisplayInterface<'a> = SPIInterface<DisplaySpi<'a>, DisplayDataCommandPin>;
pub type DisplaySpi<'d> = ExclusiveDevice<
    SpiDmaBus<'d, DisplaySpiInstance, DmaChannel0, FullDuplexMode, Async>,
    DummyOutputPin,
    Delay,
>;

pub type AdcSclk = GpioPin<6>;
pub type AdcMosi = GpioPin<7>;
pub type AdcMiso = GpioPin<5>;
pub type AdcChipSelect = GpioPin<9>;
pub type AdcClockEnable = GpioPin<23>;
pub type AdcDrdy = GpioPin<4>;
pub type AdcReset = GpioPin<15>;
pub type TouchDetect = GpioPin<2>;

pub type AdcClockEnablePin = Output<'static, AdcClockEnable>;
pub type AdcDrdyPin = Input<'static, AdcDrdy>;
pub type AdcResetPin = Output<'static, AdcReset>;
pub type TouchDetectPin = Input<'static, TouchDetect>;
pub type AdcChipSelectPin = Output<'static, AdcChipSelect>;

pub type AdcMosiPin = Output<'static, AdcMosi>;
pub type AdcMisoPin = Input<'static, AdcMiso>;
pub type AdcSclkPin = Output<'static, AdcSclk>;

pub type AdcSpi =
    ExclusiveDevice<BitbangSpi<AdcMosiPin, AdcMisoPin, AdcSclkPin>, AdcChipSelectPin, Delay>;

pub type VbusDetect = GpioPin<3>;
pub type ChargerStatus = GpioPin<20>;

pub type BatteryAdcEnablePin = DummyOutputPin;
pub type VbusDetectPin = Input<'static, VbusDetect>;
pub type ChargerStatusPin = Input<'static, ChargerStatus>;

pub type EcgFrontend = Frontend<AdcSpi, AdcDrdyPin, AdcResetPin, AdcClockEnablePin, TouchDetectPin>;
pub type PoweredEcgFrontend =
    PoweredFrontend<AdcSpi, AdcDrdyPin, AdcResetPin, AdcClockEnablePin, TouchDetectPin>;

pub type Display = DisplayType<DisplayResetPin>;

pub type BatteryFgI2cInstance = peripherals::I2C0;
pub type I2cSda = GpioPin<19>;
pub type I2cScl = GpioPin<18>;
pub type BatteryFgI2c = I2C<'static, BatteryFgI2cInstance, Async>;
pub type BatteryFg = BatteryFgType<BatteryFgI2c, BatteryAdcEnablePin>;

impl super::startup::StartupResources {
    pub async fn initialize() -> Self {
        Self::common_init();

        let peripherals = esp_hal::init({
            let mut config = esp_hal::Config::default();
            config.cpu_clock = CpuClock::max();
            config
        });

        let timg0 = TimerGroup::new(peripherals.TIMG0);
        esp_hal_embassy::init(timg0.timer0);

        let io = Io::new(peripherals.GPIO, peripherals.IO_MUX);

        let dma = Dma::new(peripherals.DMA);

        let display = Self::create_display_driver(
            dma.channel0,
            peripherals.SPI2,
            io.pins.gpio10,
            io.pins.gpio8,
            io.pins.gpio11,
            io.pins.gpio22,
            io.pins.gpio21,
        );

        let adc = Self::create_frontend_driver(
            ExclusiveDevice::new(
                BitbangSpi::new(
                    Output::new_typed(io.pins.gpio7, Level::Low),
                    Input::new_typed(io.pins.gpio5, Pull::None),
                    Output::new_typed(io.pins.gpio6, Level::Low),
                    1u32.MHz(),
                ),
                Output::new_typed(io.pins.gpio9, Level::High),
                Delay,
            ),
            io.pins.gpio4,
            io.pins.gpio15,
            io.pins.gpio23,
            io.pins.gpio2,
        );

        let battery_monitor = Self::setup_battery_monitor_fg(
            peripherals.I2C0,
            io.pins.gpio19,
            io.pins.gpio18,
            io.pins.gpio3,
            io.pins.gpio20,
            DummyOutputPin,
        )
        .await;

        let sw_int = SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);

        Self {
            display,
            frontend: adc,
            battery_monitor,
            wifi: static_cell::make_static! {
                WifiDriver::new(
                    peripherals.WIFI,
                    AnyTimer::from(SystemTimer::new(peripherals.SYSTIMER).split::<Target>().alarm0),
                    peripherals.RNG,
                    peripherals.RADIO_CLK,
                )
            },
            rtc: Rtc::new(peripherals.LPWR),
            software_interrupt1: sw_int.software_interrupt1,
        }
    }
}
