use display_interface_spi::SPIInterface;
use embassy_time::Delay;
use embedded_hal_bus::spi::ExclusiveDevice;

use crate::board::{
    drivers::{battery_monitor::BatteryMonitor, frontend::Frontend},
    utils::DummyOutputPin,
    wifi::WifiDriver,
    AdcSpi, ChargerStatusPin, Display, DisplayDmaChannel, EcgFrontend, VbusDetectPin,
};
use esp_hal::{
    clock::CpuClock,
    dma::*,
    dma_buffers,
    gpio::{Input, InputPin, Level, Output, OutputPin, Pull},
    i2c,
    interrupt::software::SoftwareInterrupt,
    peripherals::Peripherals,
    rtc_cntl::Rtc,
    spi::{
        master::{Config as SpiConfig, Spi},
        Mode,
    },
    time::Rate,
};

#[cfg(feature = "esp32s3")]
use crate::board::AdcDmaChannel;

#[cfg(feature = "battery_max17055")]
use esp_hal::i2c::master::I2c;
#[cfg(feature = "battery_max17055")]
use {
    crate::board::{BatteryAdcEnablePin, BatteryFg},
    max17055::{DesignData, Max17055},
};

pub struct StartupResources {
    pub display: Display,
    pub frontend: EcgFrontend,
    pub battery_monitor: BatteryMonitor<VbusDetectPin, ChargerStatusPin>,

    pub wifi: &'static mut WifiDriver,
    pub rtc: Rtc<'static>,

    pub software_interrupt2: SoftwareInterrupt<'static, 2>,
}

impl StartupResources {
    pub(super) fn common_init() -> Peripherals {
        esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()))
    }

    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn create_display_driver(
        display_dma_channel: DisplayDmaChannel<'static>,
        display_spi: impl esp_hal::spi::master::Instance + 'static,
        display_reset: impl OutputPin + 'static,
        display_dc: impl OutputPin + 'static,
        display_cs: impl OutputPin + 'static,
        display_sclk: impl OutputPin + 'static,
        display_mosi: impl OutputPin + 'static,
    ) -> Display {
        let (rx_buffer, rx_descriptors, tx_buffer, tx_descriptors) = dma_buffers!(4092);
        let dma_tx_buf = DmaTxBuf::new(tx_descriptors, tx_buffer).unwrap();
        let dma_rx_buf = DmaRxBuf::new(rx_descriptors, rx_buffer).unwrap();
        let display_spi = Spi::new(
            display_spi,
            SpiConfig::default()
                .with_frequency(Rate::from_mhz(40))
                .with_mode(Mode::_0),
        )
        .unwrap()
        .with_sck(display_sclk)
        .with_mosi(display_mosi)
        .with_cs(display_cs)
        .with_dma(display_dma_channel)
        .with_buffers(dma_rx_buf, dma_tx_buf)
        .into_async();

        Display::new(
            SPIInterface::new(
                ExclusiveDevice::new(display_spi, DummyOutputPin, Delay).unwrap(),
                Output::new(display_dc, Level::Low, Default::default()),
            ),
            Output::new(display_reset, Level::Low, Default::default()),
        )
    }

    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    #[cfg(feature = "esp32s3")]
    pub(crate) fn create_frontend_spi(
        adc_dma_channel: AdcDmaChannel<'static>,
        adc_spi: impl esp_hal::spi::master::Instance + 'static,
        adc_sclk: impl OutputPin + 'static,
        adc_mosi: impl OutputPin + 'static,
        adc_miso: impl InputPin + 'static,
        adc_cs: impl OutputPin + 'static,
    ) -> AdcSpi {
        use esp_hal::time::Rate;

        let (rx_buffer, rx_descriptors, tx_buffer, tx_descriptors) = dma_buffers!(4092);
        let dma_tx_buf = DmaTxBuf::new(tx_descriptors, tx_buffer).unwrap();
        let dma_rx_buf = DmaRxBuf::new(rx_descriptors, rx_buffer).unwrap();

        ExclusiveDevice::new(
            Spi::new(
                adc_spi,
                SpiConfig::default()
                    .with_frequency(Rate::from_mhz(1))
                    .with_mode(Mode::_1),
            )
            .unwrap()
            .with_sck(adc_sclk)
            .with_mosi(adc_mosi)
            .with_miso(adc_miso)
            .with_dma(adc_dma_channel)
            .with_buffers(dma_rx_buf, dma_tx_buf)
            .into_async(),
            Output::new(adc_cs, Level::High, Default::default()),
            Delay,
        )
        .unwrap()
    }

    #[inline(always)]
    pub(crate) fn create_frontend_driver(
        adc_spi: AdcSpi,
        adc_drdy: impl InputPin + 'static,
        adc_reset: impl OutputPin + 'static,
        adc_clock_enable: Option<impl OutputPin + 'static>,
        touch_detect: impl InputPin + 'static,
    ) -> EcgFrontend {
        // DRDY

        Frontend::new(
            adc_spi,
            Input::new(adc_drdy, Default::default()),
            Output::new(adc_reset, Level::Low, Default::default()),
            adc_clock_enable.map(|en| Output::new(en, Level::Low, Default::default())),
            Input::new(touch_detect, Default::default()),
        )
    }

    #[cfg(feature = "battery_max17055")]
    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn setup_battery_monitor_fg(
        i2c: impl i2c::master::Instance + 'static,
        sda: impl InputPin + OutputPin + 'static,
        scl: impl InputPin + OutputPin + 'static,
        vbus_detect: impl InputPin + 'static,
        charger_status: impl InputPin + 'static,
        fg_enable: BatteryAdcEnablePin,
    ) -> BatteryMonitor<VbusDetectPin, ChargerStatusPin> {
        // MCP73832T-2ACI/OT
        // - ITerm/Ireg = 7.5%
        // - Vreg = 4.2
        // R_prog = 4.7k
        // i_chg = 1000/4.7 = 212mA
        // i_chg_term = 212 * 0.0075 = 1.59mA
        // LSB = 1.5625μV/20mOhm = 78.125μA/LSB
        // 1.59mA / 78.125μA/LSB ~~ 20 LSB

        use esp_hal::{gpio::InputConfig, time::Rate};
        let design = DesignData {
            capacity: 320,
            i_chg_term: 20,
            v_empty: 3000,
            v_recovery: 3880,
            v_charge: 4200,
            r_sense: 20,
        };

        BatteryMonitor::start(
            Input::new(vbus_detect, Default::default()),
            Input::new(charger_status, InputConfig::default().with_pull(Pull::Up)),
            BatteryFg::new(
                Max17055::new(
                    I2c::new(
                        i2c,
                        i2c::master::Config::default().with_frequency(Rate::from_khz(100)),
                    )
                    .unwrap()
                    .with_sda(sda)
                    .with_scl(scl)
                    .into_async(),
                    design,
                ),
                fg_enable,
            ),
        )
        .await
    }
}
