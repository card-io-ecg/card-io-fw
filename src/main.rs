#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]

extern crate alloc;

use embassy_executor::{Executor, _export::StaticCell};
use esp_backtrace as _;

#[cfg(feature = "esp32s2")]
pub use esp32s2_hal as hal;

#[cfg(feature = "esp32s3")]
pub use esp32s3_hal as hal;

#[cfg(feature = "esp32s2")]
pub use esp32s2 as pac;

#[cfg(feature = "esp32s3")]
pub use esp32s3 as pac;

use esp_println::logger::init_logger;

use display_interface_spi_async::SPIInterface;
use hal::{
    clock::{ClockControl, CpuClock},
    dma::{ChannelRx, ChannelTx, DmaPriority},
    embassy,
    gdma::{Gdma, *},
    gpio::{
        Bank0GpioRegisterAccess, Floating, GpioPin, Input, InputOutputAnalogPinType, Output,
        PushPull, SingleCoreInteruptStatusRegisterAccessBank0,
    },
    peripherals::Peripherals,
    prelude::*,
    soc::gpio::*,
    spi::{dma::SpiDma, FullDuplexMode, SpiMode},
    timer::TimerGroup,
    Rtc, Spi, IO,
};

mod display;
mod frontend;
mod heap;

use crate::{display::Display, frontend::Frontend, heap::init_heap};

static EXECUTOR: StaticCell<Executor> = StaticCell::new();

type DisplaySpi<'d> = SpiDma<
    'd,
    hal::peripherals::SPI2,
    ChannelTx<'d, Channel0TxImpl, Channel0>,
    ChannelRx<'d, Channel0RxImpl, Channel0>,
    SuitablePeripheral0,
    FullDuplexMode,
>;

type DisplayDataCommand = GpioPin<
    Output<PushPull>,
    Bank0GpioRegisterAccess,
    SingleCoreInteruptStatusRegisterAccessBank0,
    InputOutputAnalogPinType,
    Gpio13Signals,
    13,
>;
type DisplayChipSelect = GpioPin<
    Output<PushPull>,
    Bank0GpioRegisterAccess,
    SingleCoreInteruptStatusRegisterAccessBank0,
    InputOutputAnalogPinType,
    Gpio10Signals,
    10,
>;
type DisplayReset = GpioPin<
    Output<PushPull>,
    Bank0GpioRegisterAccess,
    SingleCoreInteruptStatusRegisterAccessBank0,
    InputOutputAnalogPinType,
    Gpio9Signals,
    9,
>;

type DisplayInterface<'a> = SPIInterface<DisplaySpi<'a>, DisplayDataCommand, DisplayChipSelect>;

type AdcDrdy = GpioPin<
    Input<Floating>,
    Bank0GpioRegisterAccess,
    SingleCoreInteruptStatusRegisterAccessBank0,
    InputOutputAnalogPinType,
    Gpio4Signals,
    4,
>;
type AdcReset = GpioPin<
    Output<PushPull>,
    Bank0GpioRegisterAccess,
    SingleCoreInteruptStatusRegisterAccessBank0,
    InputOutputAnalogPinType,
    Gpio2Signals,
    2,
>;
type TouchDetect = GpioPin<
    Input<Floating>,
    Bank0GpioRegisterAccess,
    SingleCoreInteruptStatusRegisterAccessBank0,
    InputOutputAnalogPinType,
    Gpio1Signals,
    1,
>;
type AdcSpi<'a> = Spi<'a, hal::peripherals::SPI3, FullDuplexMode>;

struct Resources {
    display: Display<DisplayInterface<'static>, DisplayReset>,
    frontend: Frontend<AdcSpi<'static>, AdcDrdy, AdcReset, TouchDetect>,
}

#[entry]
fn main() -> ! {
    init_heap();
    init_logger(log::LevelFilter::Info);

    let peripherals = Peripherals::take();

    let mut system = peripherals.SYSTEM.split();
    let clocks = ClockControl::configure(system.clock_control, CpuClock::Clock240MHz).freeze();

    let mut rtc = Rtc::new(peripherals.RTC_CNTL);
    rtc.rwdt.disable();

    let timer_group0 = TimerGroup::new(
        peripherals.TIMG0,
        &clocks,
        &mut system.peripheral_clock_control,
    );
    embassy::init(&clocks, timer_group0.timer0);

    let io = IO::new(peripherals.GPIO, peripherals.IO_MUX);

    let dma = Gdma::new(peripherals.DMA, &mut system.peripheral_clock_control);
    let display_dma_channel = dma.channel0;

    // Display
    let display_reset = io.pins.gpio9.into_push_pull_output();
    let display_dc = io.pins.gpio13.into_push_pull_output();

    let mut display_cs = io.pins.gpio10.into_push_pull_output();
    let display_sclk = io.pins.gpio12;
    let display_mosi = io.pins.gpio11;

    let display_spi = peripherals.SPI2;

    display_cs.connect_peripheral_to_output(display_spi.cs_signal());

    static mut DISPLAY_SPI_DESCRIPTORS: [u32; 24] = [0u32; 8 * 3];
    static mut DISPLAY_SPI_RX_DESCRIPTORS: [u32; 24] = [0u32; 8 * 3];
    let display_spi = Spi::new_no_cs_no_miso(
        display_spi,
        display_sclk,
        display_mosi,
        10u32.MHz(),
        SpiMode::Mode0,
        &mut system.peripheral_clock_control,
        &clocks,
    )
    .with_dma(display_dma_channel.configure(
        false,
        unsafe { &mut DISPLAY_SPI_DESCRIPTORS },
        unsafe { &mut DISPLAY_SPI_RX_DESCRIPTORS },
        DmaPriority::Priority0,
    ));

    let display = Display::new(
        SPIInterface::new(display_spi, display_dc, display_cs),
        display_reset,
    );

    // ADC
    let adc_sclk = io.pins.gpio6;
    let adc_mosi = io.pins.gpio7;
    let adc_miso = io.pins.gpio5;
    let adc_cs = io.pins.gpio18;

    let adc_drdy = io.pins.gpio4.into_floating_input();
    let adc_reset = io.pins.gpio2.into_push_pull_output();
    let touch_detect = io.pins.gpio1.into_floating_input();

    let adc = Frontend::new(
        Spi::new(
            peripherals.SPI3,
            adc_sclk,
            adc_mosi,
            adc_miso,
            adc_cs,
            500u32.kHz(),
            SpiMode::Mode0,
            &mut system.peripheral_clock_control,
            &clocks,
        ),
        adc_drdy,
        adc_reset,
        touch_detect,
    );

    let resources = Resources {
        display,
        frontend: adc,
    };

    let executor = EXECUTOR.init(Executor::new());
    executor.run(move |spawner| {
        spawner.spawn(main_task(resources)).ok();
    });
}

enum AppState {
    Initialize,
    Measure,
    Shutdown,
}

#[embassy_executor::task]
async fn main_task(mut resources: Resources) {
    // If the device is awake, the display should be enabled.
    let mut display = resources.display.enable().await.unwrap();

    let mut state = AppState::Initialize;

    loop {
        let new_state = match state {
            AppState::Initialize => initialize(&mut display, &mut resources.frontend).await,
            AppState::Measure => measure(&mut display, &mut resources.frontend).await,
            AppState::Shutdown => break,
        };

        state = new_state;
    }

    display.shut_down();

    let (_, _, _, touch) = resources.frontend.split();
    enter_deep_sleep(touch);
}

async fn initialize(
    display: &mut display::PoweredDisplay<'_, DisplayInterface<'_>, DisplayReset>,
    frontend: &mut Frontend<AdcSpi<'_>, AdcDrdy, AdcReset, TouchDetect>,
) -> AppState {
    todo!()
}

async fn measure(
    display: &mut display::PoweredDisplay<'_, DisplayInterface<'_>, DisplayReset>,
    frontend: &mut Frontend<AdcSpi<'_>, AdcDrdy, AdcReset, TouchDetect>,
) -> AppState {
    todo!()
}

fn enter_deep_sleep(wakeup_pin: TouchDetect) -> ! {
    let rtc = unsafe { &*pac::RTC_CNTL::PTR };

    todo!()
}
