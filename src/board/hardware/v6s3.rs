use crate::board::{
    drivers::{
        battery_monitor::battery_fg::BatteryFg as BatteryFgType,
        display::Display as DisplayType,
        frontend::{Frontend, PoweredFrontend},
    },
    utils::DummyOutputPin,
    wifi::WifiDriver,
};
use display_interface_spi::{NoDelay, SPIInterface};
use embedded_hal_bus::spi::ExclusiveDevice;
use esp_hal::{
    dma::*,
    gpio::{GpioPin, Input, Io, Output},
    i2c::I2C,
    interrupt::software::SoftwareInterruptControl,
    peripherals,
    prelude::*,
    rtc_cntl::Rtc,
    spi::{master::SpiDmaBus, FullDuplexMode},
    timer::{
        systimer::{SystemTimer, Target},
        timg::TimerGroup,
        ErasedTimer,
    },
    Async,
};

pub type DisplaySpiInstance = peripherals::SPI2;
pub type DisplayDmaChannel = ChannelCreator0;
pub type DisplayDataCommand = GpioPin<17>;
pub type DisplayChipSelect = GpioPin<8>;
pub type DisplayReset = GpioPin<18>;
pub type DisplaySclk = GpioPin<39>;
pub type DisplayMosi = GpioPin<38>;

pub type DisplayResetPin = Output<'static, DisplayReset>;
pub type DisplayDataCommandPin = Output<'static, DisplayDataCommand>;

pub type DisplayInterface<'a> = SPIInterface<DisplaySpi<'a>, DisplayDataCommandPin>;
pub type DisplaySpi<'d> = ExclusiveDevice<
    SpiDmaBus<'d, DisplaySpiInstance, DmaChannel0, FullDuplexMode, Async>,
    DummyOutputPin,
    NoDelay,
>;

pub type AdcDmaChannel = ChannelCreator1;
pub type AdcSpiInstance = peripherals::SPI3;
pub type AdcSclk = GpioPin<6>;
pub type AdcMosi = GpioPin<7>;
pub type AdcMiso = GpioPin<5>;
pub type AdcChipSelect = GpioPin<0>;
pub type AdcClockEnable = GpioPin<40>;
pub type AdcDrdy = GpioPin<4>;
pub type AdcReset = GpioPin<42>;
pub type TouchDetect = GpioPin<1>;

pub type AdcClockEnablePin = Output<'static, AdcClockEnable>;
pub type AdcDrdyPin = Input<'static, AdcDrdy>;
pub type AdcResetPin = Output<'static, AdcReset>;
pub type TouchDetectPin = Input<'static, TouchDetect>;
pub type AdcChipSelectPin = Output<'static, AdcChipSelect>;

pub type AdcSpi = ExclusiveDevice<
    SpiDmaBus<'static, AdcSpiInstance, DmaChannel1, FullDuplexMode, Async>,
    AdcChipSelectPin,
    NoDelay,
>;

pub type VbusDetect = GpioPin<2>;
pub type ChargerStatus = GpioPin<37>;

pub type BatteryAdcEnablePin = DummyOutputPin;
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

        let peripherals = esp_hal::init({
            let mut config = esp_hal::Config::default();
            config.cpu_clock = CpuClock::max();
            config
        });

        let systimer = SystemTimer::new(peripherals.SYSTIMER).split::<Target>();
        esp_hal_embassy::init(systimer.alarm0);

        let io = Io::new(peripherals.GPIO, peripherals.IO_MUX);

        let dma = Dma::new(peripherals.DMA);

        let display = Self::create_display_driver(
            dma.channel0,
            peripherals.SPI2,
            io.pins.gpio18,
            io.pins.gpio17,
            io.pins.gpio8,
            io.pins.gpio39,
            io.pins.gpio38,
        );

        let adc = Self::create_frontend_driver(
            Self::create_frontend_spi(
                dma.channel1,
                peripherals.SPI3,
                io.pins.gpio6,
                io.pins.gpio7,
                io.pins.gpio5,
                io.pins.gpio0,
            ),
            io.pins.gpio4,
            io.pins.gpio42,
            io.pins.gpio40,
            io.pins.gpio1,
        );

        let battery_monitor = Self::setup_battery_monitor_fg(
            peripherals.I2C0,
            io.pins.gpio36,
            io.pins.gpio35,
            io.pins.gpio2,
            io.pins.gpio37,
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
                    ErasedTimer::from(TimerGroup::new(peripherals.TIMG0)
                        .timer0),
                    peripherals.RNG,
                    peripherals.RADIO_CLK,
                )
            },
            rtc: Rtc::new(peripherals.LPWR),
            software_interrupt1: sw_int.software_interrupt1,
        }
    }
}
