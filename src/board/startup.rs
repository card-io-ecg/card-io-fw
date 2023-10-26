use display_interface_spi::SPIInterface;
use embassy_executor::_export::StaticCell;
use embassy_time::Delay;
use embedded_hal::digital::OutputPin;
use embedded_hal_bus::spi::ExclusiveDevice;

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
        AdcSpiInstance, Display, DisplayChipSelect, DisplayDataCommand, DisplayDmaChannel,
        DisplayMosi, DisplayReset, DisplaySclk, DisplaySpiInstance, EcgFrontend, MiscPins,
        TouchDetect,
    },
    heap::init_heap,
};
#[cfg(feature = "log")]
use esp_println::logger::init_logger;
use fugit::RateExtU32;

pub static WIFI_DRIVER: StaticCell<WifiDriver> = StaticCell::new();

pub struct StartupResources {
    pub display: Display,
    pub frontend: EcgFrontend,
    pub clocks: Clocks<'static>,
    #[cfg(feature = "battery_adc")]
    pub battery_adc: BatteryAdc,

    #[cfg(feature = "battery_max17055")]
    pub battery_fg: BatteryFg,

    pub misc_pins: MiscPins,
    pub wifi: &'static mut WifiDriver,
    pub rtc: Rtc<'static>,
}

impl StartupResources {
    pub(super) fn common_init() {
        #[cfg(feature = "log")]
        init_logger(log::LevelFilter::Trace); // we let the compile-time log level filter do the work
        init_heap();
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
}
