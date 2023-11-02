use display_interface_spi::SPIInterface;
use embassy_executor::_export::StaticCell;
use embassy_time::Delay;
use embedded_hal::digital::OutputPin;
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
                master::{
                    dma::{WithDmaSpi2, WithDmaSpi3},
                    Instance, Spi,
                },
                SpiMode,
            },
            Rtc,
        },
        utils::DummyOutputPin,
        wifi::WifiDriver,
        AdcChipSelect, AdcClockEnable, AdcDmaChannel, AdcDrdy, AdcMiso, AdcMosi, AdcReset, AdcSclk,
        AdcSpiInstance, ChargerStatus, Display, DisplayChipSelect, DisplayDataCommand,
        DisplayDmaChannel, DisplayMosi, DisplayReset, DisplaySclk, DisplaySpiInstance, EcgFrontend,
        TouchDetect, VbusDetect,
    },
    heap::init_heap,
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

pub static WIFI_DRIVER: StaticCell<WifiDriver> = StaticCell::new();

pub struct StartupResources {
    pub display: Display,
    pub frontend: EcgFrontend,
    pub clocks: Clocks<'static>,
    pub battery_monitor: BatteryMonitor<VbusDetect, ChargerStatus>,

    pub wifi: &'static mut WifiDriver,
    pub rtc: Rtc<'static>,
}

impl StartupResources {
    pub(super) fn common_init() {
        #[cfg(feature = "log")]
        init_logger(log::LevelFilter::Trace); // we let the compile-time log level filter do the work
        init_heap();

        use core::ptr::addr_of;

        extern "C" {
            static mut _stack_start_cpu0: u8;
            static mut _stack_end_cpu0: u8;
        }

        // We only use a single core for now, so we can write both stack regions.
        let stack_start = unsafe { addr_of!(_stack_start_cpu0) as usize };
        let stack_end = unsafe { addr_of!(_stack_end_cpu0) as usize };
        let _stack_protection = make_static!(crate::stack_protection::StackMonitor::protect(
            stack_start..stack_end
        ));
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

        static mut DISPLAY_SPI_DESCRIPTORS: [u32; 3] = [0; 3];
        static mut DISPLAY_SPI_RX_DESCRIPTORS: [u32; 3] = [0; 3];
        let display_spi = Spi::new_no_cs_no_miso(
            display_spi,
            display_sclk.into(),
            display_mosi.into(),
            40u32.MHz(),
            SpiMode::Mode0,
            clocks,
        )
        .with_dma(display_dma_channel.configure(
            false,
            unsafe { &mut DISPLAY_SPI_DESCRIPTORS },
            unsafe { &mut DISPLAY_SPI_RX_DESCRIPTORS },
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
    pub(crate) fn create_frontend_driver(
        adc_dma_channel: AdcDmaChannel,
        dma_in_interrupt: peripherals::Interrupt,
        dma_out_interrupt: peripherals::Interrupt,
        adc_spi: AdcSpiInstance,
        adc_sclk: impl Into<AdcSclk>,
        adc_mosi: impl Into<AdcMosi>,
        adc_miso: impl Into<AdcMiso>,
        adc_drdy: impl Into<AdcDrdy>,
        adc_reset: impl Into<AdcReset>,
        adc_clock_enable: impl Into<AdcClockEnable>,
        touch_detect: impl Into<TouchDetect>,
        adc_cs: impl Into<AdcChipSelect>,

        clocks: &Clocks,
    ) -> EcgFrontend {
        unwrap!(interrupt::enable(
            dma_in_interrupt,
            interrupt::Priority::Priority1
        ));
        unwrap!(interrupt::enable(
            dma_out_interrupt,
            interrupt::Priority::Priority1
        ));

        // DRDY
        unwrap!(interrupt::enable(
            peripherals::Interrupt::GPIO,
            interrupt::Priority::Priority3,
        ));

        let mut adc_cs: AdcChipSelect = adc_cs.into();

        unwrap!(adc_cs.set_high().ok());

        static mut ADC_SPI_DESCRIPTORS: [u32; 3] = [0; 3];
        static mut ADC_SPI_RX_DESCRIPTORS: [u32; 3] = [0; 3];
        Frontend::new(
            ExclusiveDevice::new(
                Spi::new_no_cs(
                    adc_spi,
                    adc_sclk.into(),
                    adc_mosi.into(),
                    adc_miso.into(),
                    1u32.MHz(),
                    SpiMode::Mode1,
                    clocks,
                )
                .with_dma(adc_dma_channel.configure(
                    false,
                    unsafe { &mut ADC_SPI_DESCRIPTORS },
                    unsafe { &mut ADC_SPI_RX_DESCRIPTORS },
                    DmaPriority::Priority1,
                )),
                adc_cs,
                Delay,
            ),
            adc_drdy.into(),
            adc_reset.into(),
            adc_clock_enable.into(),
            touch_detect.into(),
        )
    }

    #[cfg(feature = "battery_max17055")]
    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn setup_batter_monitor_fg(
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
