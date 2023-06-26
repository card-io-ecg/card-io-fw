#[cfg(feature = "battery_adc")]
use crate::board::{
    drivers::battery_adc::BatteryAdc as BatteryAdcType,
    hal::{adc::ADC1, gpio::Analog},
};

#[cfg(feature = "battery_max17055")]
use crate::board::{drivers::battery_fg::BatteryFg as BatteryFgType, hal::i2c::I2C};
#[cfg(feature = "battery_max17055")]
use max17055::{DesignData, Max17055};

use crate::{
    board::{
        drivers::{
            display::{Display as DisplayType, PoweredDisplay as PoweredDisplayType},
            frontend::{Frontend, PoweredFrontend},
        },
        hal::{
            self,
            clock::{ClockControl, CpuClock},
            dma::{ChannelRx, ChannelTx, DmaPriority},
            embassy,
            gdma::*,
            gpio::{Floating, GpioPin, Input, Output, PullUp, PushPull},
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
pub type DisplayChipSelect = GpioPin<Output<PushPull>, 11>;
pub type DisplayReset = GpioPin<Output<PushPull>, 12>;
pub type DisplaySclk = GpioPin<Output<PushPull>, 14>;
pub type DisplayMosi = GpioPin<Output<PushPull>, 21>;

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
pub type AdcClockEnable = GpioPin<Output<PushPull>, 38>;

#[cfg(feature = "battery_adc")]
pub type BatteryAdcInput = GpioPin<Analog, 9>;
#[cfg(any(feature = "battery_adc", feature = "battery_max17055"))]
pub type BatteryAdcEnable = GpioPin<Output<PushPull>, 8>;
pub type VbusDetect = GpioPin<Input<Floating>, 17>;
#[cfg(feature = "battery_adc")]
pub type ChargeCurrentInput = GpioPin<Analog, 10>;
pub type ChargerStatus = GpioPin<Input<PullUp>, 47>;

pub type EcgFrontend = Frontend<AdcSpi<'static>, AdcDrdy, AdcReset, AdcClockEnable, TouchDetect>;
pub type PoweredEcgFrontend =
    PoweredFrontend<AdcSpi<'static>, AdcDrdy, AdcReset, AdcClockEnable, TouchDetect>;

pub type Display = DisplayType<DisplayInterface<'static>, DisplayReset>;
pub type PoweredDisplay = PoweredDisplayType<DisplayInterface<'static>, DisplayReset>;

#[cfg(feature = "battery_adc")]
pub type BatteryAdc = BatteryAdcType<BatteryAdcInput, ChargeCurrentInput, BatteryAdcEnable, ADC1>;

#[cfg(feature = "battery_max17055")]
pub type BatteryFgI2c = I2C<'static, hal::peripherals::I2C0>;
#[cfg(feature = "battery_max17055")]
pub type BatteryFg = BatteryFgType<BatteryFgI2c, BatteryAdcEnable>;

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
            io.pins.gpio12.into_push_pull_output(),
            io.pins.gpio13.into_push_pull_output(),
            io.pins.gpio11.into_push_pull_output(),
            io.pins.gpio14.into_push_pull_output(),
            io.pins.gpio21.into_push_pull_output(),
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
        let adc_clock_enable = io.pins.gpio38.into_push_pull_output();
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

        #[cfg(feature = "battery_adc")]
        let battery_adc = {
            // Battery measurement
            let batt_adc_in = io.pins.gpio9.into_analog();
            let batt_adc_en = io.pins.gpio8.into_push_pull_output();

            let chg_current = io.pins.gpio10.into_analog();

            // Battery ADC
            let analog = peripherals.SENS.split();

            BatteryAdc::new(analog.adc1, batt_adc_in, chg_current, batt_adc_en)
        };

        #[cfg(feature = "battery_max17055")]
        let battery_fg = {
            let i2c0 = I2C::new(
                peripherals.I2C0,
                io.pins.gpio35,
                io.pins.gpio36,
                100u32.kHz(),
                &mut system.peripheral_clock_control,
                &clocks,
            );

            interrupt::enable(
                peripherals::Interrupt::I2C_EXT0,
                interrupt::Priority::Priority1,
            )
            .unwrap();

            // MCP73832T-2ACI/OT
            // - ITerm/Ireg = 7.5%
            // - Vreg = 4.2
            // R_prog = 4.7k
            // i_chg = 1000/4.7 = 212mA
            // i_chg_term = 212 * 0.0075 = 1.59mA
            // LSB = 1.5625μV/20mOhm = 78.125μA/LSB

            let batt_pullup_en = io.pins.gpio8.into_push_pull_output();

            let design = DesignData {
                capacity: 320,
                i_chg_term: 20, // 1.5625mA
                v_empty: 300,
                v_recovery: 97, // 3880mV
                v_charge: 4200,
                r_sense: 20,
            };
            BatteryFg::new(Max17055::new(i2c0, design), batt_pullup_en)
        };

        // Charger
        let vbus_detect = io.pins.gpio17.into_floating_input();
        let chg_status = io.pins.gpio47.into_pull_up_input();

        // Wifi
        let (wifi, _) = peripherals.RADIO.split();

        Self {
            display,
            frontend: adc,
            #[cfg(feature = "battery_adc")]
            battery_adc,
            #[cfg(feature = "battery_max17055")]
            battery_fg,
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
