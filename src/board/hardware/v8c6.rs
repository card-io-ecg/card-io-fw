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
    interrupt::software::SoftwareInterruptControl,
    peripherals::{DMA_CH0, GPIO2},
    rtc_cntl::Rtc,
    spi::master::SpiDmaBus,
    time::Rate,
    timer::systimer::SystemTimer,
    Async,
};

pub const TOUCH_PIN: u8 = 4;
pub const VBUS_DETECT_PIN: u8 = 7;

pub type DisplayDmaChannel<'a> = DMA_CH0<'a>;

pub type DisplaySpi<'d> = ExclusiveDevice<SpiDmaBus<'d, Async>, DummyOutputPin, Delay>;

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

        let sw_int = SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);

        let systimer = SystemTimer::new(peripherals.SYSTIMER);
        esp_rtos::start(systimer.alarm0, sw_int.software_interrupt0);

        let display = Self::create_display_driver(
            peripherals.DMA_CH0,
            peripherals.SPI2,
            peripherals.GPIO23,
            peripherals.GPIO8,
            peripherals.GPIO17,
            peripherals.GPIO10,
            peripherals.GPIO16,
        );

        let frontend = Self::create_frontend_driver(
            ExclusiveDevice::new(
                BitbangSpi::new(
                    Output::new(peripherals.GPIO6, Level::Low, Default::default()),
                    Input::new(peripherals.GPIO3, Default::default()),
                    Output::new(peripherals.GPIO5, Level::Low, Default::default()),
                    Rate::from_mhz(1),
                ),
                Output::new(peripherals.GPIO9, Level::High, Default::default()),
                Delay,
            )
            .unwrap(),
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
            rtc: Rtc::new(peripherals.LPWR),
            software_interrupt2: sw_int.software_interrupt2,
        }
    }
}
