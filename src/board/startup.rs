use display_interface_spi::SPIInterface;

#[cfg(feature = "battery_adc")]
use crate::board::BatteryAdc;
#[cfg(feature = "battery_max17055")]
use crate::board::BatteryFg;

use crate::board::{
    hal::{
        clock::Clocks, dma::DmaPriority, interrupt, peripherals, prelude::*, spi::SpiMode,
        system::PeripheralClockControl, Spi,
    },
    utils::{DummyOutputPin, SpiDeviceWrapper},
    wifi::driver::WifiDriver,
    Display, DisplayChipSelect, DisplayDataCommand, DisplayDmaChannel, DisplayMosi, DisplayReset,
    DisplaySclk, DisplaySpiInstance, EcgFrontend, MiscPins,
};

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
    pub wifi: WifiDriver,
}

impl StartupResources {
    #[inline(always)]
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
}
