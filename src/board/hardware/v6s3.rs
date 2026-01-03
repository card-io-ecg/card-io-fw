use crate::board::{
    drivers::{
        battery_monitor::battery_fg::BatteryFg as BatteryFgType,
        display::Display as DisplayType,
        frontend::{Frontend, PoweredFrontend},
    },
    utils::DummyOutputPin,
};
use display_interface_spi::SPIInterface;
use embassy_time::Delay;
use embedded_hal_bus::spi::ExclusiveDevice;
use esp_hal::{
    gpio::{Input, Output},
    i2c::master::I2c,
    interrupt::software::SoftwareInterruptControl,
    peripherals::{DMA_CH0, DMA_CH1},
    rtc_cntl::Rtc,
    spi::master::SpiDmaBus,
    timer::systimer::SystemTimer,
    Async,
};

pub const TOUCH_PIN: u8 = 1;
pub const VBUS_DETECT_PIN: u8 = 2;

pub type DisplayDmaChannel<'a> = DMA_CH0<'a>;

pub type DisplayInterface<'a> = SPIInterface<DisplaySpi<'a>, Output<'static>>;
pub type DisplaySpi<'d> = ExclusiveDevice<SpiDmaBus<'d, Async>, DummyOutputPin, Delay>;

pub type AdcDmaChannel<'a> = DMA_CH1<'a>;

pub type AdcSpi = ExclusiveDevice<SpiDmaBus<'static, Async>, Output<'static>, Delay>;

pub type BatteryAdcEnablePin = DummyOutputPin;
pub type VbusDetectPin = Input<'static>;
pub type ChargerStatusPin = Input<'static>;

pub type EcgFrontend = Frontend<AdcSpi, Input<'static>, Output<'static>>;
pub type PoweredEcgFrontend = PoweredFrontend<AdcSpi, Input<'static>, Output<'static>>;

pub type Display = DisplayType<Output<'static>>;

pub type BatteryFgI2c = I2c<'static, Async>;
pub type BatteryFg = BatteryFgType<BatteryFgI2c, BatteryAdcEnablePin>;

impl super::startup::StartupResources {
    pub async fn initialize() -> Self {
        let peripherals = Self::common_init();

        let systimer = SystemTimer::new(peripherals.SYSTIMER);
        esp_rtos::start(systimer.alarm0);

        let display = Self::create_display_driver(
            peripherals.DMA_CH0,
            peripherals.SPI2,
            peripherals.GPIO18,
            peripherals.GPIO17,
            peripherals.GPIO8,
            peripherals.GPIO39,
            peripherals.GPIO38,
        );

        let frontend = Self::create_frontend_driver(
            Self::create_frontend_spi(
                peripherals.DMA_CH1,
                peripherals.SPI3,
                peripherals.GPIO6,
                peripherals.GPIO7,
                peripherals.GPIO5,
                peripherals.GPIO0,
            ),
            peripherals.GPIO4,
            peripherals.GPIO42,
            Some(peripherals.GPIO40),
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
            frontend,
            battery_monitor,
            #[cfg(feature = "wifi")]
            wifi: peripherals.WIFI,
            rtc: Rtc::new(peripherals.LPWR),
            software_interrupt2: sw_int.software_interrupt2,
        }
    }
}
