use display_interface_spi::SPIInterface;
use embassy_time::Delay;
use embedded_hal_bus::spi::ExclusiveDevice;
use static_cell::make_static;

use crate::{
    board::{
        drivers::{battery_monitor::BatteryMonitor, frontend::Frontend},
        utils::DummyOutputPin,
        wifi::WifiDriver,
        AdcClockEnable, AdcDrdy, AdcReset, AdcSpi, ChargerStatus, ChargerStatusPin, Display,
        DisplayChipSelect, DisplayDataCommand, DisplayDmaChannel, DisplayMosi, DisplayReset,
        DisplaySclk, DisplaySpiInstance, EcgFrontend, TouchDetect, VbusDetect, VbusDetectPin,
    },
    heap::init_heap,
};
use esp_hal::{
    clock::Clocks,
    dma::{DmaDescriptor, DmaPriority},
    gpio::{Input, Level, Output, Pull},
    rtc_cntl::Rtc,
    spi::{
        master::{dma::*, Spi},
        SpiMode,
    },
    system::SoftwareInterrupt,
};

#[cfg(feature = "esp32s3")]
use crate::board::{AdcChipSelect, AdcDmaChannel, AdcMiso, AdcMosi, AdcSclk, AdcSpiInstance};

#[cfg(feature = "battery_max17055")]
use esp_hal::i2c::I2C;
#[cfg(feature = "battery_max17055")]
use {
    crate::board::{BatteryAdcEnable, BatteryFg, BatteryFgI2cInstance, I2cScl, I2cSda},
    max17055::{DesignData, Max17055},
};

#[cfg(feature = "log")]
use esp_println::logger::init_logger;

use fugit::RateExtU32;

pub struct StartupResources {
    pub display: Display,
    pub frontend: EcgFrontend,
    pub clocks: Clocks<'static>,
    pub battery_monitor: BatteryMonitor<VbusDetectPin, ChargerStatusPin>,

    pub wifi: &'static mut WifiDriver,
    pub rtc: Rtc<'static>,

    pub software_interrupt1: SoftwareInterrupt<2>,
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
        display_spi: DisplaySpiInstance,
        display_reset: DisplayReset,
        display_dc: DisplayDataCommand,
        display_cs: DisplayChipSelect,
        display_sclk: DisplaySclk,
        display_mosi: DisplayMosi,
        clocks: &Clocks,
    ) -> Display {
        let display_spi = Spi::new(display_spi, 40u32.MHz(), SpiMode::Mode0, clocks)
            .with_sck(display_sclk)
            .with_mosi(display_mosi)
            .with_cs(display_cs)
            .with_dma(
                display_dma_channel.configure_for_async(false, DmaPriority::Priority0),
                make_static!([DmaDescriptor::EMPTY; 1]),
                make_static!([DmaDescriptor::EMPTY; 1]),
            );

        Display::new(
            SPIInterface::new(
                ExclusiveDevice::new(display_spi, DummyOutputPin, Delay),
                Output::new(display_dc, Level::Low),
            ),
            Output::new(display_reset, Level::Low),
        )
    }

    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    #[cfg(feature = "esp32s3")]
    pub(crate) fn create_frontend_spi(
        adc_dma_channel: AdcDmaChannel,
        adc_spi: AdcSpiInstance,
        adc_sclk: AdcSclk,
        adc_mosi: AdcMosi,
        adc_miso: AdcMiso,
        adc_cs: AdcChipSelect,

        clocks: &Clocks,
    ) -> AdcSpi {
        ExclusiveDevice::new(
            Spi::new(adc_spi, 1u32.MHz(), SpiMode::Mode1, clocks)
                .with_sck(adc_sclk)
                .with_mosi(adc_mosi)
                .with_miso(adc_miso)
                .with_dma(
                    adc_dma_channel.configure_for_async(false, DmaPriority::Priority1),
                    make_static!([DmaDescriptor::EMPTY; 1]),
                    make_static!([DmaDescriptor::EMPTY; 1]),
                ),
            Output::new(adc_cs, Level::High),
            Delay,
        )
    }

    #[inline(always)]
    pub(crate) fn create_frontend_driver(
        adc_spi: AdcSpi,
        adc_drdy: AdcDrdy,
        adc_reset: AdcReset,
        adc_clock_enable: AdcClockEnable,
        touch_detect: TouchDetect,
    ) -> EcgFrontend {
        // DRDY

        Frontend::new(
            adc_spi,
            Input::new(adc_drdy, Pull::None),
            Output::new(adc_reset, Level::Low),
            Output::new(adc_clock_enable, Level::Low),
            Input::new(touch_detect, Pull::None),
        )
    }

    #[cfg(feature = "battery_max17055")]
    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn setup_battery_monitor_fg(
        i2c: BatteryFgI2cInstance,
        sda: I2cSda,
        scl: I2cScl,
        vbus_detect: VbusDetect,
        charger_status: ChargerStatus,
        fg_enable: BatteryAdcEnable,
        clocks: &Clocks<'_>,
    ) -> BatteryMonitor<VbusDetectPin, ChargerStatusPin> {
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
            Input::new(vbus_detect, Pull::None),
            Input::new(charger_status, Pull::None),
            BatteryFg::new(
                Max17055::new(I2C::new_async(i2c, sda, scl, 100u32.kHz(), clocks), design),
                Output::new(fg_enable, Level::Low),
            ),
        )
        .await
    }
}
