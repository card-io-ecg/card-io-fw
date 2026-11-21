use crate::board::{
    drivers::{
        frontend::{Frontend, PoweredFrontend},
        parl_io_spi::ParlIoSpi,
    },
    utils::{BlockingDelay, DummyOutputPin},
};
use embassy_time::Delay;
use embedded_hal_bus::spi::ExclusiveDevice;
use esp_hal::{
    clock::ll::LpSlowClkConfig,
    gpio::{Input, Output},
    i2c::master::I2c,
    peripherals::{DMA_CH0, GPIO2},
    spi::master::Spi,
    timer::systimer::SystemTimer,
    Async,
};

pub const TOUCH_PIN: u8 = 4;
pub const VBUS_DETECT_PIN: u8 = 7;
pub const RC_CLK_SOURCE: LpSlowClkConfig = LpSlowClkConfig::Xtal32k;

pub type DisplayDmaChannel<'a> = DMA_CH0<'a>;

pub type DisplaySpi<'d> = ExclusiveDevice<ParlIoSpi<'d>, DummyOutputPin, Delay>;

pub type AdcSpi = ExclusiveDevice<Spi<'static, Async>, Output<'static>, BlockingDelay>;

pub type BatteryAdcEnablePin = DummyOutputPin;
pub type VbusDetectPin = Input<'static>;
pub type ChargerStatusPin = Input<'static>;

pub type EcgFrontend = Frontend<AdcSpi, Input<'static>, Output<'static>>;
pub type PoweredEcgFrontend = PoweredFrontend<AdcSpi, Input<'static>, Output<'static>>;

pub type BatteryFgI2c = I2c<'static, Async>;

impl super::startup::StartupResources {
    pub async fn initialize() -> Self {
        let peripherals = Self::common_init();

        let systimer = SystemTimer::new(peripherals.SYSTIMER);
        esp_rtos::start_with_idle_hook(systimer.alarm0, super::setup_sleep(peripherals.LPWR));

        let display = Self::create_display_driver(
            Self::create_display_spi(
                peripherals.DMA_CH0,
                peripherals.PARL_IO,
                peripherals.GPIO17,
                peripherals.GPIO10,
                peripherals.GPIO16,
            ),
            peripherals.GPIO23,
            peripherals.GPIO8,
        );

        let frontend = Self::create_frontend_driver(
            Self::create_frontend_spi(
                peripherals.SPI2,
                peripherals.GPIO5,
                peripherals.GPIO6,
                peripherals.GPIO3,
                peripherals.GPIO9,
            ),
            peripherals.GPIO2,
            peripherals.GPIO15,
            None::<GPIO2>,
            peripherals.GPIO4,
        );

        let battery_monitor = Self::setup_battery_monitor_fg(
            peripherals.I2C0,
            peripherals.GPIO19,
            peripherals.GPIO18,
            peripherals.GPIO7,
            peripherals.GPIO20,
            DummyOutputPin,
        )
        .await;

        Self {
            display,
            frontend,
            battery_monitor,
            #[cfg(feature = "wifi")]
            wifi: peripherals.WIFI,
            software_interrupt2: peripherals.FROM_CPU_INTR2,
            flash: peripherals.FLASH,
            clock: Self::create_clock(peripherals.RTC_TIMER),
        }
    }
}
