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
    i2c::master::I2c,
    interrupt::software::SoftwareInterruptControl,
    peripherals,
    rtc_cntl::Rtc,
    spi::master::SpiDmaBus,
    timer::{
        systimer::{SystemTimer, Target},
        timg::TimerGroup,
        AnyTimer,
    },
    Async,
};

pub type DisplayDmaChannel = ChannelCreator<0>;

pub type DisplayInterface<'a> = SPIInterface<DisplaySpi<'a>, Output<'static>>;
pub type DisplaySpi<'d> = ExclusiveDevice<SpiDmaBus<'d, Async>, DummyOutputPin, Delay>;

pub type AdcDmaChannel = ChannelCreator<1>;

pub type AdcSpi = ExclusiveDevice<SpiDmaBus<'static, Async>, Output<'static>, Delay>;

pub type BatteryAdcEnablePin = DummyOutputPin;
pub type VbusDetectPin = Input<'static>;
pub type ChargerStatusPin = Input<'static>;

pub type EcgFrontend =
    Frontend<AdcSpi, Input<'static>, Output<'static>, Output<'static>, Input<'static>>;
pub type PoweredEcgFrontend =
    PoweredFrontend<AdcSpi, Input<'static>, Output<'static>, Output<'static>, Input<'static>>;

pub type Display = DisplayType<Output<'static>>;

pub type BatteryFgI2c = I2c<'static, Async>;
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
            peripherals.GPIO18,
            peripherals.GPIO17,
            peripherals.GPIO8,
            peripherals.GPIO39,
            peripherals.GPIO38,
        );

        let adc = Self::create_frontend_driver(
            Self::create_frontend_spi(
                dma.channel1,
                peripherals.SPI3,
                peripherals.GPIO6,
                peripherals.GPIO7,
                peripherals.GPIO5,
                peripherals.GPIO0,
            ),
            peripherals.GPIO4,
            peripherals.GPIO42,
            peripherals.GPIO40,
            peripherals.GPIO1,
        );

        let battery_monitor = Self::setup_battery_monitor_fg(
            peripherals.I2C0,
            peripherals.GPIO36,
            peripherals.GPIO35,
            peripherals.GPIO2,
            peripherals.GPIO37,
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
