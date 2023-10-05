use crate::board::{
    drivers::{
        battery_monitor::battery_adc::BatteryAdc as BatteryAdcType,
        display::{Display as DisplayType, PoweredDisplay as PoweredDisplayType},
        frontend::{Frontend, PoweredFrontend},
    },
    hal::{
        self,
        adc::ADC2,
        clock::{ClockControl, CpuClock},
        embassy,
        gdma::*,
        gpio::{Analog, Floating, GpioPin, Input, Output, PullUp, PushPull},
        peripherals::{self, Peripherals},
        prelude::*,
        spi::{dma::SpiDma, FullDuplexMode},
        systimer::SystemTimer,
        Rtc, IO,
    },
    startup::WIFI_DRIVER,
    utils::DummyOutputPin,
    wifi::WifiDriver,
    *,
};
use embassy_time::Delay;
use embedded_hal_bus::spi::ExclusiveDevice;

use display_interface_spi::SPIInterface;

pub type DisplaySpiInstance = hal::peripherals::SPI2;
pub type DisplayDmaChannel = ChannelCreator0;
pub type DisplayDataCommand = GpioPin<Output<PushPull>, 13>;
pub type DisplayChipSelect = GpioPin<Output<PushPull>, 10>;
pub type DisplayReset = GpioPin<Output<PushPull>, 9>;
pub type DisplaySclk = GpioPin<Output<PushPull>, 12>;
pub type DisplayMosi = GpioPin<Output<PushPull>, 11>;

pub type DisplayInterface<'a> = SPIInterface<DisplaySpi<'a>, DisplayDataCommand>;
pub type DisplaySpi<'d> = ExclusiveDevice<
    SpiDma<'d, DisplaySpiInstance, Channel0, FullDuplexMode>,
    DummyOutputPin,
    Delay,
>;

pub type AdcDmaChannel = ChannelCreator1;
pub type AdcSpiInstance = hal::peripherals::SPI3;
pub type AdcSclk = GpioPin<Output<PushPull>, 6>;
pub type AdcMosi = GpioPin<Output<PushPull>, 7>;
pub type AdcMiso = GpioPin<Input<Floating>, 5>;
pub type AdcChipSelect = GpioPin<Output<PushPull>, 18>;
pub type AdcClockEnable = DummyOutputPin;
pub type AdcDrdy = GpioPin<Input<Floating>, 4>;
pub type AdcReset = GpioPin<Output<PushPull>, 2>;
pub type TouchDetect = GpioPin<Input<Floating>, 1>;
pub type AdcSpi<'d> =
    ExclusiveDevice<SpiDma<'d, AdcSpiInstance, Channel1, FullDuplexMode>, AdcChipSelect, Delay>;

pub type BatteryAdcInput = GpioPin<Analog, 17>;
pub type BatteryAdcEnable = GpioPin<Output<PushPull>, 8>;
pub type VbusDetect = GpioPin<Input<Floating>, 47>;
pub type ChargeCurrentInput = GpioPin<Analog, 14>;
pub type ChargerStatus = GpioPin<Input<PullUp>, 21>;

pub type EcgFrontend = Frontend<AdcSpi<'static>, AdcDrdy, AdcReset, AdcClockEnable, TouchDetect>;
pub type PoweredEcgFrontend =
    PoweredFrontend<AdcSpi<'static>, AdcDrdy, AdcReset, AdcClockEnable, TouchDetect>;

pub type Display = DisplayType<DisplayReset>;
pub type PoweredDisplay = PoweredDisplayType<DisplayReset>;

pub type BatteryAdc = BatteryAdcType<BatteryAdcInput, ChargeCurrentInput, BatteryAdcEnable, ADC2>;

impl super::startup::StartupResources {
    pub fn initialize() -> Self {
        Self::common_init();

        let peripherals = Peripherals::take();

        let mut system = peripherals.SYSTEM.split();
        let clocks = ClockControl::configure(system.clock_control, CpuClock::Clock240MHz).freeze();

        embassy::init(&clocks, SystemTimer::new(peripherals.SYSTIMER));

        let io = IO::new(peripherals.GPIO, peripherals.IO_MUX);

        let dma = Gdma::new(peripherals.DMA, &mut system.peripheral_clock_control);

        let display = Self::create_display_driver(
            dma.channel0,
            peripherals::Interrupt::DMA_IN_CH0,
            peripherals::Interrupt::DMA_OUT_CH0,
            peripherals.SPI2,
            io.pins.gpio9,
            io.pins.gpio13,
            io.pins.gpio10,
            io.pins.gpio12,
            io.pins.gpio11,
            &mut system.peripheral_clock_control,
            &clocks,
        );

        let adc = Self::create_frontend_driver(
            dma.channel1,
            peripherals::Interrupt::DMA_IN_CH1,
            peripherals::Interrupt::DMA_OUT_CH1,
            peripherals.SPI3,
            io.pins.gpio6,
            io.pins.gpio7,
            io.pins.gpio5,
            io.pins.gpio4,
            io.pins.gpio2,
            DummyOutputPin,
            io.pins.gpio1,
            io.pins.gpio18,
            &mut system.peripheral_clock_control,
            &clocks,
        );

        // Battery ADC
        let analog = peripherals.SENS.split();

        let battery_adc =
            BatteryAdc::new(analog.adc2, io.pins.gpio17, io.pins.gpio14, io.pins.gpio8);

        // Wifi
        let (wifi, _) = peripherals.RADIO.split();

        Self {
            display,
            frontend: adc,
            battery_adc,
            wifi: WIFI_DRIVER.init_with(|| {
                WifiDriver::new(
                    wifi,
                    peripherals.TIMG1,
                    peripherals.RNG,
                    system.radio_clock_control,
                    &clocks,
                    &mut system.peripheral_clock_control,
                )
            }),
            clocks,
            peripheral_clock_control: system.peripheral_clock_control,
            rtc: Rtc::new(peripherals.RTC_CNTL),

            misc_pins: MiscPins {
                vbus_detect: io.pins.gpio47.into(),
                chg_status: io.pins.gpio21.into(),
            },
        }
    }
}
