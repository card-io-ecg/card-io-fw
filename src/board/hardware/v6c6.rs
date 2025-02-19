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
    gpio::{Input, Level, Output},
    i2c::master::I2c,
    interrupt::software::SoftwareInterruptControl,
    rtc_cntl::Rtc,
    spi::master::SpiDmaBus,
    time::Rate,
    timer::{systimer::SystemTimer, AnyTimer},
    Async,
};
use static_cell::StaticCell;

pub use crate::board::drivers::bitbang_spi::BitbangSpi;

pub type DisplayDmaChannel = DmaChannel0;

pub type DisplayInterface<'a> = SPIInterface<DisplaySpi<'a>, Output<'static>>;
pub type DisplaySpi<'d> = ExclusiveDevice<SpiDmaBus<'d, Async>, DummyOutputPin, Delay>;

pub type AdcSpi = ExclusiveDevice<
    BitbangSpi<Output<'static>, Input<'static>, Output<'static>>,
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
            peripherals.GPIO10,
            peripherals.GPIO8,
            peripherals.GPIO11,
            peripherals.GPIO22,
            peripherals.GPIO21,
        );

        let adc = Self::create_frontend_driver(
            ExclusiveDevice::new(
                BitbangSpi::new(
                    Output::new(peripherals.GPIO7, Level::Low, Default::default()),
                    Input::new(peripherals.GPIO5, Default::default()),
                    Output::new(peripherals.GPIO6, Level::Low, Default::default()),
                    Rate::from_mhz(1),
                ),
                Output::new(peripherals.GPIO9, Level::High, Default::default()),
                Delay,
            )
            .unwrap(),
            peripherals.GPIO4,
            peripherals.GPIO15,
            peripherals.GPIO23,
            peripherals.GPIO2,
        );

        let battery_monitor = Self::setup_battery_monitor_fg(
            peripherals.I2C0,
            peripherals.GPIO19,
            peripherals.GPIO18,
            peripherals.GPIO3,
            peripherals.GPIO20,
            DummyOutputPin,
        )
        .await;

        let sw_int = SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);

        static WIFI: StaticCell<WifiDriver> = StaticCell::new();
        let wifi = WIFI.init(WifiDriver::new(
            peripherals.WIFI,
            AnyTimer::from(systimer.alarm2),
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
