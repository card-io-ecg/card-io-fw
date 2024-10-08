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
    gpio::{Input, Io, Output},
    i2c::I2C,
    interrupt::software::SoftwareInterruptControl,
    peripherals,
    rtc_cntl::Rtc,
    spi::{master::SpiDmaBus, FullDuplexMode},
    timer::{
        systimer::{SystemTimer, Target},
        timg::TimerGroup,
        AnyTimer,
    },
    Async,
};

pub type DisplaySpiInstance = peripherals::SPI2;
pub type DisplayDmaChannel = ChannelCreator<0>;

pub type DisplayInterface<'a> = SPIInterface<DisplaySpi<'a>, Output<'static>>;
pub type DisplaySpi<'d> = ExclusiveDevice<
    SpiDmaBus<'d, DisplaySpiInstance, DmaChannel0, FullDuplexMode, Async>,
    DummyOutputPin,
    Delay,
>;

pub type AdcDmaChannel = ChannelCreator<1>;
pub type AdcSpiInstance = peripherals::SPI3;

pub type AdcSpi = ExclusiveDevice<
    SpiDmaBus<'static, AdcSpiInstance, DmaChannel1, FullDuplexMode, Async>,
    Output<'static>,
    Delay,
>;

pub type BatteryAdcEnablePin = DummyOutputPin;
pub type VbusDetectPin = Input<'static>;
pub type ChargerStatusPin = Input<'static>;

pub type EcgFrontend =
    Frontend<AdcSpi, Input<'static>, Output<'static>, Output<'static>, Input<'static>>;
pub type PoweredEcgFrontend =
    PoweredFrontend<AdcSpi, Input<'static>, Output<'static>, Output<'static>, Input<'static>>;

pub type Display = DisplayType<Output<'static>>;

pub type BatteryFgI2cInstance = peripherals::I2C0;
pub type BatteryFgI2c = I2C<'static, BatteryFgI2cInstance, Async>;
pub type BatteryFg = BatteryFgType<BatteryFgI2c, BatteryAdcEnablePin>;

impl super::startup::StartupResources {
    pub async fn initialize() -> Self {
        let peripherals = Self::common_init();

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
                    AnyTimer::from(TimerGroup::new(peripherals.TIMG0)
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
