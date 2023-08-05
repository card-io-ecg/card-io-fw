use display_interface_spi::SPIInterface;
use embassy_executor::_export::StaticCell;

#[cfg(feature = "battery_adc")]
use crate::board::BatteryAdc;
#[cfg(feature = "battery_max17055")]
use crate::board::BatteryFg;

use crate::{
    board::{
        drivers::frontend::Frontend,
        hal::{
            clock::Clocks,
            dma::DmaPriority,
            interrupt, peripherals,
            prelude::*,
            spi::{dma::WithDmaSpi3, SpiMode},
            system::PeripheralClockControl,
            Spi,
        },
        utils::{DummyOutputPin, SpiDeviceWrapper},
        wifi::WifiDriver,
        AdcChipSelect, AdcClockEnable, AdcDmaChannel, AdcDrdy, AdcMiso, AdcMosi, AdcReset, AdcSclk,
        AdcSpiInstance, Display, DisplayChipSelect, DisplayDataCommand, DisplayDmaChannel,
        DisplayMosi, DisplayReset, DisplaySclk, DisplaySpiInstance, EcgFrontend, MiscPins,
        TouchDetect,
    },
    heap::init_heap,
};
use esp_println::logger::init_logger;

pub static WIFI_DRIVER: StaticCell<WifiDriver> = StaticCell::new();

pub struct StartupResources {
    pub display: Display,
    pub frontend: EcgFrontend,
    pub clocks: Clocks<'static>,
    pub peripheral_clock_control: PeripheralClockControl,
    #[cfg(feature = "battery_adc")]
    pub battery_adc: BatteryAdc,

    #[cfg(feature = "battery_max17055")]
    pub battery_fg: BatteryFg,

    pub misc_pins: MiscPins,
    pub wifi: &'static mut WifiDriver,
}

impl StartupResources {
    pub(super) fn common_init() {
        init_logger(log::LevelFilter::Info);
        init_heap();
    }

    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn create_display_driver(
        display_dma_channel: DisplayDmaChannel,
        dma_in_interrupt: peripherals::Interrupt,
        dma_out_interrupt: peripherals::Interrupt,
        display_spi: DisplaySpiInstance,
        display_reset: DisplayReset,
        display_dc: DisplayDataCommand,
        mut display_cs: DisplayChipSelect,
        display_sclk: DisplaySclk,
        display_mosi: DisplayMosi,
        pcc: &mut PeripheralClockControl,
        clocks: &Clocks,
    ) -> Display {
        interrupt::enable(dma_in_interrupt, interrupt::Priority::Priority1).unwrap();
        interrupt::enable(dma_out_interrupt, interrupt::Priority::Priority1).unwrap();

        display_cs.connect_peripheral_to_output(display_spi.cs_signal());

        static mut DISPLAY_SPI_DESCRIPTORS: [u32; 3] = [0; 3];
        static mut DISPLAY_SPI_RX_DESCRIPTORS: [u32; 3] = [0; 3];
        let display_spi = Spi::new_no_cs_no_miso(
            display_spi,
            display_sclk,
            display_mosi,
            40u32.MHz(),
            SpiMode::Mode0,
            pcc,
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
                SpiDeviceWrapper::new(display_spi, DummyOutputPin),
                display_dc,
            ),
            display_reset,
        )
    }

    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn create_frontend_driver(
        adc_dma_channel: AdcDmaChannel,
        dma_in_interrupt: peripherals::Interrupt,
        dma_out_interrupt: peripherals::Interrupt,
        adc_spi: AdcSpiInstance,
        adc_sclk: AdcSclk,
        adc_mosi: AdcMosi,
        adc_miso: AdcMiso,

        adc_drdy: AdcDrdy,
        adc_reset: AdcReset,
        adc_clock_enable: AdcClockEnable,
        touch_detect: TouchDetect,
        mut adc_cs: AdcChipSelect,

        pcc: &mut PeripheralClockControl,
        clocks: &Clocks,
    ) -> EcgFrontend {
        interrupt::enable(dma_in_interrupt, interrupt::Priority::Priority1).unwrap();
        interrupt::enable(dma_out_interrupt, interrupt::Priority::Priority1).unwrap();

        adc_cs.set_high().unwrap();

        static mut ADC_SPI_DESCRIPTORS: [u32; 3] = [0; 3];
        static mut ADC_SPI_RX_DESCRIPTORS: [u32; 3] = [0; 3];
        Frontend::new(
            SpiDeviceWrapper::new(
                Spi::new_no_cs(
                    adc_spi,
                    adc_sclk,
                    adc_mosi,
                    adc_miso,
                    1u32.MHz(),
                    SpiMode::Mode1,
                    pcc,
                    clocks,
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
        )
    }
}
