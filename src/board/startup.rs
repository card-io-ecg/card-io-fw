use display_interface_spi::SPIInterface;
use embassy_time::Delay;
use embedded_hal_bus::spi::ExclusiveDevice;
use static_cell::make_static;

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
    peripheral::Peripheral,
    peripherals::Peripherals,
    rtc_cntl::Rtc,
    spi::{
        master::{Config as SpiConfig, Spi},
        SpiMode,
    },
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

use fugit::RateExtU32;

pub struct StartupResources {
    pub display: Display,
    pub frontend: EcgFrontend,
    pub battery_monitor: BatteryMonitor<VbusDetectPin, ChargerStatusPin>,

    pub wifi: &'static mut WifiDriver,
    pub rtc: Rtc<'static>,

    pub software_interrupt1: SoftwareInterrupt<1>,
}

impl StartupResources {
    pub(super) fn common_init() -> Peripherals {
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

        esp_hal::init({
            let mut config = esp_hal::Config::default();
            config.cpu_clock = CpuClock::max();
            config
        })
    }

    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn create_display_driver(
        display_dma_channel: DisplayDmaChannel,
        display_spi: impl Peripheral<P = impl esp_hal::spi::master::Instance> + 'static,
        display_reset: impl Peripheral<P = impl OutputPin> + 'static,
        display_dc: impl Peripheral<P = impl OutputPin> + 'static,
        display_cs: impl Peripheral<P = impl OutputPin> + 'static,
        display_sclk: impl Peripheral<P = impl OutputPin> + 'static,
        display_mosi: impl Peripheral<P = impl OutputPin> + 'static,
    ) -> Display {
        let (rx_buffer, rx_descriptors, tx_buffer, tx_descriptors) = dma_buffers!(4092);
        let dma_tx_buf = DmaTxBuf::new(tx_descriptors, tx_buffer).unwrap();
        let dma_rx_buf = DmaRxBuf::new(rx_descriptors, rx_buffer).unwrap();
        let display_spi = Spi::new_with_config(display_spi, {
            let mut config = SpiConfig::default();
            config.frequency = 40u32.MHz();
            config.mode = SpiMode::Mode0;
            config
        })
        .with_sck(display_sclk)
        .with_mosi(display_mosi)
        .with_cs(display_cs)
        .with_dma(display_dma_channel.configure(false, DmaPriority::Priority0))
        .with_buffers(dma_rx_buf, dma_tx_buf)
        .into_async();

        Display::new(
            SPIInterface::new(
                ExclusiveDevice::new(display_spi, DummyOutputPin, Delay).unwrap(),
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
        adc_spi: impl Peripheral<P = impl esp_hal::spi::master::Instance> + 'static,
        adc_sclk: impl Peripheral<P = impl OutputPin> + 'static,
        adc_mosi: impl Peripheral<P = impl OutputPin> + 'static,
        adc_miso: impl Peripheral<P = impl OutputPin> + 'static,
        adc_cs: impl Peripheral<P = impl OutputPin> + 'static,
    ) -> AdcSpi {
        let (rx_buffer, rx_descriptors, tx_buffer, tx_descriptors) = dma_buffers!(4092);
        let dma_tx_buf = DmaTxBuf::new(tx_descriptors, tx_buffer).unwrap();
        let dma_rx_buf = DmaRxBuf::new(rx_descriptors, rx_buffer).unwrap();

        ExclusiveDevice::new(
            Spi::new_with_config(adc_spi, {
                let mut config = SpiConfig::default();
                config.frequency = 1u32.MHz();
                config.mode = SpiMode::Mode1;
                config
            })
            .with_sck(adc_sclk)
            .with_mosi(adc_mosi)
            .with_miso(adc_miso)
            .with_dma(adc_dma_channel.configure(false, DmaPriority::Priority1))
            .with_buffers(dma_rx_buf, dma_tx_buf)
            .into_async(),
            Output::new(adc_cs, Level::High),
            Delay,
        )
        .unwrap()
    }

    #[inline(always)]
    pub(crate) fn create_frontend_driver(
        adc_spi: AdcSpi,
        adc_drdy: impl Peripheral<P = impl InputPin> + 'static,
        adc_reset: impl Peripheral<P = impl OutputPin> + 'static,
        adc_clock_enable: impl Peripheral<P = impl OutputPin> + 'static,
        touch_detect: impl Peripheral<P = impl InputPin> + 'static,
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
    pub(crate) async fn setup_battery_monitor_fg<
        SDA: InputPin + OutputPin,
        SCL: InputPin + OutputPin,
    >(
        i2c: impl Peripheral<P = impl i2c::master::Instance> + 'static,
        sda: impl Peripheral<P = SDA> + 'static,
        scl: impl Peripheral<P = SCL> + 'static,
        vbus_detect: impl Peripheral<P = impl InputPin> + 'static,
        charger_status: impl Peripheral<P = impl InputPin> + 'static,
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
            Input::new(charger_status, Pull::Up),
            BatteryFg::new(
                Max17055::new(
                    I2c::new(
                        i2c,
                        i2c::master::Config {
                            frequency: 100u32.kHz(),
                            ..Default::default()
                        },
                    )
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
