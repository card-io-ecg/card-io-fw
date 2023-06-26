use crate::{
    board::{
        drivers::{
            battery_adc::BatteryAdc as BatteryAdcType,
            display::{Display as DisplayType, PoweredDisplay as PoweredDisplayType},
            frontend::{Frontend, PoweredFrontend},
        },
        hal::{
            self,
            adc::ADC2,
            clock::{ClockControl, CpuClock},
            dma::{ChannelRx, ChannelTx, DmaPriority},
            embassy,
            gdma::*,
            gpio::{Analog, Floating, GpioPin, Input, Output, PullUp, PushPull},
            interrupt,
            peripherals::{self, Peripherals},
            prelude::*,
            spi::{
                dma::{SpiDma, WithDmaSpi3},
                FullDuplexMode, SpiMode,
            },
            systimer::SystemTimer,
            Rtc, Spi, IO,
        },
        utils::{DummyOutputPin, SpiDeviceWrapper},
        wifi::driver::WifiDriver,
        *,
    },
    heap::init_heap,
};

use display_interface_spi::SPIInterface;
use esp_println::logger::init_logger;

pub type DisplaySpi<'d> = SpiDeviceWrapper<
    SpiDma<
        'd,
        DisplaySpiInstance,
        ChannelTx<'d, Channel0TxImpl, Channel0>,
        ChannelRx<'d, Channel0RxImpl, Channel0>,
        SuitablePeripheral0,
        FullDuplexMode,
    >,
    DummyOutputPin,
>;

pub type DisplaySpiInstance = hal::peripherals::SPI2;
pub type DisplayDmaChannel = ChannelCreator0;
pub type DisplayDataCommand = GpioPin<Output<PushPull>, 13>;
pub type DisplayChipSelect = GpioPin<Output<PushPull>, 10>;
pub type DisplayReset = GpioPin<Output<PushPull>, 9>;
pub type DisplaySclk = GpioPin<Output<PushPull>, 12>;
pub type DisplayMosi = GpioPin<Output<PushPull>, 11>;

pub type DisplayInterface<'a> = SPIInterface<DisplaySpi<'a>, DisplayDataCommand>;

pub type AdcDrdy = GpioPin<Input<Floating>, 4>;
pub type AdcReset = GpioPin<Output<PushPull>, 2>;
pub type TouchDetect = GpioPin<Input<Floating>, 1>;
pub type AdcChipSelect = GpioPin<Output<PushPull>, 18>;
pub type AdcSpi<'d> = SpiDeviceWrapper<
    SpiDma<
        'd,
        hal::peripherals::SPI3,
        ChannelTx<'d, Channel1TxImpl, Channel1>,
        ChannelRx<'d, Channel1RxImpl, Channel1>,
        SuitablePeripheral1,
        FullDuplexMode,
    >,
    AdcChipSelect,
>;
pub type AdcClockEnable = DummyOutputPin;

pub type BatteryAdcInput = GpioPin<Analog, 17>;
pub type BatteryAdcEnable = GpioPin<Output<PushPull>, 8>;
pub type VbusDetect = GpioPin<Input<Floating>, 47>;
pub type ChargeCurrentInput = GpioPin<Analog, 14>;
pub type ChargerStatus = GpioPin<Input<PullUp>, 21>;

pub type EcgFrontend = Frontend<AdcSpi<'static>, AdcDrdy, AdcReset, AdcClockEnable, TouchDetect>;
pub type PoweredEcgFrontend =
    PoweredFrontend<AdcSpi<'static>, AdcDrdy, AdcReset, AdcClockEnable, TouchDetect>;

pub type Display = DisplayType<DisplayInterface<'static>, DisplayReset>;
pub type PoweredDisplay = PoweredDisplayType<DisplayInterface<'static>, DisplayReset>;

pub type BatteryAdc = BatteryAdcType<BatteryAdcInput, ChargeCurrentInput, BatteryAdcEnable, ADC2>;

impl super::startup::StartupResources {
    pub fn initialize() -> Self {
        init_logger(log::LevelFilter::Info);
        init_heap();

        let peripherals = Peripherals::take();

        let mut system = peripherals.SYSTEM.split();
        let clocks = ClockControl::configure(system.clock_control, CpuClock::Clock240MHz).freeze();

        let mut rtc = Rtc::new(peripherals.RTC_CNTL);
        rtc.rwdt.disable();

        embassy::init(&clocks, SystemTimer::new(peripherals.SYSTIMER));

        let io = IO::new(peripherals.GPIO, peripherals.IO_MUX);

        let dma = Gdma::new(peripherals.DMA, &mut system.peripheral_clock_control);

        let display = Self::create_display_driver(
            dma.channel0,
            peripherals::Interrupt::DMA_IN_CH0,
            peripherals::Interrupt::DMA_OUT_CH0,
            peripherals.SPI2,
            io.pins.gpio9.into_push_pull_output(),
            io.pins.gpio13.into_push_pull_output(),
            io.pins.gpio10.into_push_pull_output(),
            io.pins.gpio12.into_push_pull_output(),
            io.pins.gpio11.into_push_pull_output(),
            &mut system.peripheral_clock_control,
            &clocks,
        );

        // ADC
        let adc_dma_channel = dma.channel1;
        interrupt::enable(
            peripherals::Interrupt::DMA_IN_CH1,
            interrupt::Priority::Priority2,
        )
        .unwrap();
        interrupt::enable(
            peripherals::Interrupt::DMA_OUT_CH1,
            interrupt::Priority::Priority2,
        )
        .unwrap();

        let adc_sclk = io.pins.gpio6;
        let adc_mosi = io.pins.gpio7;
        let adc_miso = io.pins.gpio5;

        let adc_drdy = io.pins.gpio4.into_floating_input();
        let adc_reset = io.pins.gpio2.into_push_pull_output();
        let adc_clock_enable = DummyOutputPin;
        let touch_detect = io.pins.gpio1.into_floating_input();
        let mut adc_cs = io.pins.gpio18.into_push_pull_output();

        adc_cs.set_high().unwrap();

        static mut ADC_SPI_DESCRIPTORS: [u32; 3] = [0; 3];
        static mut ADC_SPI_RX_DESCRIPTORS: [u32; 3] = [0; 3];
        let adc = Frontend::new(
            SpiDeviceWrapper::new(
                Spi::new_no_cs(
                    peripherals.SPI3,
                    adc_sclk,
                    adc_mosi,
                    adc_miso,
                    1u32.MHz(),
                    SpiMode::Mode1,
                    &mut system.peripheral_clock_control,
                    &clocks,
                )
                .with_dma(adc_dma_channel.configure(
                    false,
                    unsafe { &mut ADC_SPI_DESCRIPTORS },
                    unsafe { &mut ADC_SPI_RX_DESCRIPTORS },
                    DmaPriority::Priority1,
                )),
                adc_cs,
            ),
            adc_drdy,
            adc_reset,
            adc_clock_enable,
            touch_detect,
        );

        // Battery measurement
        let batt_adc_in = io.pins.gpio17.into_analog();
        let batt_adc_en = io.pins.gpio8.into_push_pull_output();

        // Charger
        let vbus_detect = io.pins.gpio47.into_floating_input();
        let chg_current = io.pins.gpio14.into_analog();
        let chg_status = io.pins.gpio21.into_pull_up_input();

        // Battery ADC
        let analog = peripherals.SENS.split();

        let battery_adc = BatteryAdc::new(analog.adc2, batt_adc_in, chg_current, batt_adc_en);

        // Wifi
        let (wifi, _) = peripherals.RADIO.split();

        Self {
            display,
            frontend: adc,
            battery_adc,
            wifi: WifiDriver::new(
                wifi,
                peripherals.TIMG1,
                peripherals.RNG,
                system.radio_clock_control,
            ),
            clocks,
            peripheral_clock_control: system.peripheral_clock_control,

            misc_pins: MiscPins {
                vbus_detect,
                chg_status,
            },
        }
    }
}
