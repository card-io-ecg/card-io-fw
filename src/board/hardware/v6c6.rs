use crate::board::{
    drivers::{
        bitbang_spi::BitbangSpi,
        frontend::{Frontend, PoweredFrontend},
    },
    utils::DummyOutputPin,
};
use embassy_time::Delay;
use embedded_hal_bus::spi::ExclusiveDevice;
use esp_hal::{
    gpio::{Input, Level, Output},
    i2c::master::I2c,
    peripherals::DMA_CH0,
    spi::master::SpiDma,
    time::Rate,
    timer::systimer::SystemTimer,
    Async,
};

pub const TOUCH_PIN: u8 = 2;
pub const VBUS_DETECT_PIN: u8 = 3;

pub type DisplayDmaChannel<'a> = DMA_CH0<'a>;

pub type DisplaySpi<'d> = ExclusiveDevice<SpiDma<'d, Async>, DummyOutputPin, Delay>;

pub type AdcSpi = ExclusiveDevice<
    BitbangSpi<Output<'static>, Input<'static>, Output<'static>>,
    Output<'static>,
    Delay,
>;

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
        let sleep = esp_rtos::sleep::configure(peripherals.LPWR);
        esp_rtos::start_with_idle_hook(
            systimer.alarm0,
            peripherals.FROM_CPU_INTR0,
            sleep.light_sleep_hook,
        );

        let display = Self::create_display_driver(
            peripherals.DMA_CH0,
            peripherals.SPI2,
            peripherals.GPIO10,
            peripherals.GPIO8,
            peripherals.GPIO11,
            peripherals.GPIO22,
            peripherals.GPIO21,
        );

        let frontend = Self::create_frontend_driver(
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
            Some(peripherals.GPIO23),
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

        Self {
            display,
            frontend,
            battery_monitor,
            #[cfg(feature = "wifi")]
            wifi: peripherals.WIFI,
            low_power: sleep.deep_sleep,
            software_interrupt2: peripherals.FROM_CPU_INTR2,
        }
    }
}
