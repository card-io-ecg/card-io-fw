use display_interface_spi::SPIInterface;
use embassy_time::Delay;
use embedded_hal_bus::spi::ExclusiveDevice;
use static_cell::make_static;

use crate::{
    board::{
        drivers::{battery_monitor::BatteryMonitor, frontend::Frontend},
        hal::{
            clock::Clocks,
            dma::DmaPriority,
            gpio::OutputPin as _,
            interrupt, peripherals,
            spi::{
                master::{dma::*, Instance, Spi},
                SpiMode,
            },
        },
        utils::DummyOutputPin,
        wifi::WifiDriver,
        AdcClockEnable, AdcDrdy, AdcReset, AdcSpi, ChargerStatus, Display, DisplayChipSelect,
        DisplayDataCommand, DisplayDmaChannel, DisplayMosi, DisplayReset, DisplaySclk,
        DisplaySpiInstance, EcgFrontend, TouchDetect, VbusDetect,
    },
    heap::init_heap,
};

#[cfg(feature = "esp32s3")]
use crate::board::{
    hal::Rtc, AdcChipSelect, AdcDmaChannel, AdcMiso, AdcMosi, AdcSclk, AdcSpiInstance,
};

#[cfg(feature = "battery_max17055")]
use {
    crate::{
        board::{BatteryAdcEnable, BatteryFg, BatteryFgI2cInstance, I2cScl, I2cSda},
        hal::i2c::I2C,
    },
    max17055::{DesignData, Max17055},
};

#[cfg(feature = "log")]
use esp_println::logger::init_logger;

use fugit::RateExtU32;

pub struct StartupResources {
    pub display: Display,
    pub frontend: EcgFrontend,
    pub clocks: Clocks<'static>,
    pub battery_monitor: BatteryMonitor<VbusDetect, ChargerStatus>,

    pub wifi: &'static mut WifiDriver,
    #[cfg(feature = "esp32s3")]
    pub rtc: Rtc<'static>,
}

impl StartupResources {
    pub(super) fn common_init() {
        #[cfg(feature = "log")]
        init_logger(log::LevelFilter::Trace); // we let the compile-time log level filter do the work
        init_heap();

        use core::ptr::addr_of;

        #[cfg(feature = "esp32s3")]
        let stack_range = {
            extern "C" {
                static mut _stack_start_cpu0: u8;
                static mut _stack_end_cpu0: u8;
            }

            // We only use a single core for now, so we can write both stack regions.
            let stack_start = unsafe { addr_of!(_stack_start_cpu0) as usize };
            let stack_end = unsafe { addr_of!(_stack_end_cpu0) as usize };

            stack_start..stack_end
        };
        #[cfg(feature = "esp32c6")]
        let stack_range = {
            extern "C" {
                static mut _stack_start: u8;
                static mut _stack_end: u8;
            }

            // We only use a single core for now, so we can write both stack regions.
            let stack_start = unsafe { addr_of!(_stack_start) as usize };
            let stack_end = unsafe { addr_of!(_stack_end) as usize };

            stack_start..stack_end
        };
        let _stack_protection =
            make_static!(crate::stack_protection::StackMonitor::protect(stack_range));
    }

    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn create_display_driver(
        display_dma_channel: DisplayDmaChannel,
        dma_in_interrupt: peripherals::Interrupt,
        dma_out_interrupt: peripherals::Interrupt,
        display_spi: DisplaySpiInstance,
        display_reset: impl Into<DisplayReset>,
        display_dc: impl Into<DisplayDataCommand>,
        display_cs: impl Into<DisplayChipSelect>,
        display_sclk: impl Into<DisplaySclk>,
        display_mosi: impl Into<DisplayMosi>,
        clocks: &Clocks,
    ) -> Display {
        unwrap!(interrupt::enable(
            dma_in_interrupt,
            interrupt::Priority::Priority1
        ));
        unwrap!(interrupt::enable(
            dma_out_interrupt,
            interrupt::Priority::Priority1
        ));

        let mut display_cs: DisplayChipSelect = display_cs.into();

        display_cs.connect_peripheral_to_output(display_spi.cs_signal());

        const DESCR_SET_COUNT: usize = 1;
        static mut DISP_SPI_DESCRIPTORS: [u32; DESCR_SET_COUNT * 3] = [0; DESCR_SET_COUNT * 3];
        static mut DISP_SPI_RX_DESCRIPTORS: [u32; DESCR_SET_COUNT * 3] = [0; DESCR_SET_COUNT * 3];
        let display_spi = Spi::new(display_spi, 40u32.MHz(), SpiMode::Mode0, clocks)
            .with_sck(display_sclk.into())
            .with_mosi(display_mosi.into())
            .with_dma(display_dma_channel.configure(
                false,
                unsafe { &mut DISP_SPI_DESCRIPTORS },
                unsafe { &mut DISP_SPI_RX_DESCRIPTORS },
                DmaPriority::Priority0,
            ));

        Display::new(
            SPIInterface::new(
                ExclusiveDevice::new(display_spi, DummyOutputPin, Delay),
                display_dc.into(),
            ),
            display_reset.into(),
        )
    }

    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    #[cfg(feature = "esp32s3")]
    pub(crate) fn create_frontend_spi(
        adc_dma_channel: AdcDmaChannel,
        dma_in_interrupt: peripherals::Interrupt,
        dma_out_interrupt: peripherals::Interrupt,
        adc_spi: AdcSpiInstance,
        adc_sclk: impl Into<AdcSclk>,
        adc_mosi: impl Into<AdcMosi>,
        adc_miso: impl Into<AdcMiso>,
        adc_cs: impl Into<AdcChipSelect>,

        clocks: &Clocks,
    ) -> AdcSpi {
        use embedded_hal::digital::OutputPin;

        unwrap!(interrupt::enable(
            dma_in_interrupt,
            interrupt::Priority::Priority1
        ));
        unwrap!(interrupt::enable(
            dma_out_interrupt,
            interrupt::Priority::Priority1
        ));

        let mut adc_cs: AdcChipSelect = adc_cs.into();

        unwrap!(adc_cs.set_high().ok());

        const DESCR_SET_COUNT: usize = 1;
        static mut ADC_SPI_DESCRIPTORS: [u32; DESCR_SET_COUNT * 3] = [0; DESCR_SET_COUNT * 3];
        static mut ADC_SPI_RX_DESCRIPTORS: [u32; DESCR_SET_COUNT * 3] = [0; DESCR_SET_COUNT * 3];

        ExclusiveDevice::new(
            Spi::new(adc_spi, 1u32.MHz(), SpiMode::Mode1, clocks)
                .with_sck(adc_sclk.into())
                .with_mosi(adc_mosi.into())
                .with_miso(adc_miso.into())
                .with_dma(adc_dma_channel.configure(
                    false,
                    unsafe { &mut ADC_SPI_DESCRIPTORS },
                    unsafe { &mut ADC_SPI_RX_DESCRIPTORS },
                    DmaPriority::Priority1,
                )),
            adc_cs,
            Delay,
        )
    }

    #[inline(always)]
    pub(crate) fn create_frontend_driver(
        adc_spi: AdcSpi,
        adc_drdy: impl Into<AdcDrdy>,
        adc_reset: impl Into<AdcReset>,
        adc_clock_enable: impl Into<AdcClockEnable>,
        touch_detect: impl Into<TouchDetect>,
    ) -> EcgFrontend {
        // DRDY
        unwrap!(interrupt::enable(
            peripherals::Interrupt::GPIO,
            interrupt::Priority::Priority3,
        ));

        Frontend::new(
            adc_spi,
            adc_drdy.into(),
            adc_reset.into(),
            adc_clock_enable.into(),
            touch_detect.into(),
        )
    }

    #[cfg(feature = "battery_max17055")]
    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn setup_battery_monitor_fg(
        i2c: BatteryFgI2cInstance,
        i2c_interrupt: peripherals::Interrupt,
        sda: impl Into<I2cSda>,
        scl: impl Into<I2cScl>,
        vbus_detect: impl Into<VbusDetect>,
        charger_status: impl Into<ChargerStatus>,
        fg_enable: impl Into<BatteryAdcEnable>,
        clocks: &Clocks<'_>,
    ) -> BatteryMonitor<VbusDetect, ChargerStatus> {
        unwrap!(interrupt::enable(
            i2c_interrupt,
            interrupt::Priority::Priority1,
        ));

        // MCP73832T-2ACI/OT
        // - ITerm/Ireg = 7.5%
        // - Vreg = 4.2
        // R_prog = 4.7k
        // i_chg = 1000/4.7 = 212mA
        // i_chg_term = 212 * 0.0075 = 1.59mA
        // LSB = 1.5625μV/20mOhm = 78.125μA/LSB
        // 1.59mA / 78.125μA/LSB ~~ 20 LSB
        let design = DesignData {
            capacity: 320,
            i_chg_term: 20,
            v_empty: 3000,
            v_recovery: 3880,
            v_charge: 4200,
            r_sense: 20,
        };

        BatteryMonitor::start(
            vbus_detect.into(),
            charger_status.into(),
            BatteryFg::new(
                Max17055::new(
                    I2C::new(i2c, sda.into(), scl.into(), 100u32.kHz(), clocks),
                    design,
                ),
                fg_enable.into(),
            ),
        )
        .await
    }
}
