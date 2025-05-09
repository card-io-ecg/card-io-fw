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
    gpio::{Input, Level, Output},
    i2c::master::I2c,
    interrupt::software::SoftwareInterruptControl,
    peripherals::{DMA_CH0, DMA_CH1},
    rtc_cntl::Rtc,
    spi::master::SpiDmaBus,
    timer::{systimer::SystemTimer, timg::TimerGroup, AnyTimer},
    Async,
};
use static_cell::StaticCell;

use display_interface_spi::SPIInterface;

pub type DisplayDmaChannel = DMA_CH0<'static>;

pub type DisplayInterface<'a> = SPIInterface<DisplaySpi<'a>, Output<'static>>;
pub type DisplaySpi<'d> = ExclusiveDevice<SpiDmaBus<'d, Async>, DummyOutputPin, Delay>;

pub type AdcDmaChannel = DMA_CH1<'static>;

pub type AdcSpi = ExclusiveDevice<SpiDmaBus<'static, Async>, Output<'static>, Delay>;

pub type BatteryAdcEnablePin = Output<'static>;
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

        let systimer = SystemTimer::new(peripherals.SYSTIMER);
        esp_hal_embassy::init([
            AnyTimer::from(systimer.alarm0),
            AnyTimer::from(systimer.alarm1),
        ]);

        let display = Self::create_display_driver(
            peripherals.DMA_CH0,
            peripherals.SPI2,
            peripherals.GPIO12,
            peripherals.GPIO13,
            peripherals.GPIO11,
            peripherals.GPIO14,
            peripherals.GPIO21,
        );

        let adc = Self::create_frontend_driver(
            Self::create_frontend_spi(
                peripherals.DMA_CH1,
                peripherals.SPI3,
                peripherals.GPIO6,
                peripherals.GPIO7,
                peripherals.GPIO5,
                peripherals.GPIO18,
            ),
            peripherals.GPIO4,
            peripherals.GPIO2,
            peripherals.GPIO38,
            peripherals.GPIO1,
        );

        let battery_monitor = Self::setup_battery_monitor_fg(
            peripherals.I2C0,
            peripherals.GPIO36,
            peripherals.GPIO35,
            peripherals.GPIO17,
            peripherals.GPIO47,
            Output::new(peripherals.GPIO8, Level::Low, Default::default()),
        )
        .await;

        let sw_int = SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);

        static WIFI: StaticCell<WifiDriver> = StaticCell::new();
        let wifi = WIFI.init(WifiDriver::new(
            peripherals.WIFI,
            AnyTimer::from(TimerGroup::new(peripherals.TIMG0).timer0),
            peripherals.RNG,
            peripherals.RADIO_CLK,
        ));

        Self {
            display,
            frontend: adc,
            battery_monitor,
            wifi,
            rtc: Rtc::new(peripherals.LPWR),
            software_interrupt1: sw_int.software_interrupt1,
        }
    }
}
