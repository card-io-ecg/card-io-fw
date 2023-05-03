#![no_std]
#![no_main]
#![feature(async_fn_in_trait)]
#![feature(type_alias_impl_trait)]
#![feature(let_chains)]
#![feature(associated_type_bounds)]
#![allow(incomplete_features)]

extern crate alloc;

use embassy_executor::{Executor, _export::StaticCell};
use embassy_time::{Duration, Instant, Ticker};
use embedded_graphics::{pixelcolor::BinaryColor, prelude::DrawTarget};
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

use core::fmt::Debug;
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
mod spi_device;

use crate::{display::Display, frontend::Frontend, heap::init_heap, spi_device::SpiDeviceWrapper};

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
type AdcChipSelect = GpioPin<
    Output<PushPull>,
    Bank0GpioRegisterAccess,
    SingleCoreInteruptStatusRegisterAccessBank0,
    InputOutputAnalogPinType,
    Gpio18Signals,
    18,
>;
type AdcSpi<'a> = SpiDeviceWrapper<Spi<'a, hal::peripherals::SPI3, FullDuplexMode>, AdcChipSelect>;

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

    let adc_cs = io.pins.gpio18.into_push_pull_output();
    let adc_drdy = io.pins.gpio4.into_floating_input();
    let adc_reset = io.pins.gpio2.into_push_pull_output();
    let touch_detect = io.pins.gpio1.into_floating_input();

    let adc = Frontend::new(
        SpiDeviceWrapper {
            spi: Spi::new_no_cs(
                peripherals.SPI3,
                adc_sclk,
                adc_mosi,
                adc_miso,
                500u32.kHz(),
                SpiMode::Mode0,
                &mut system.peripheral_clock_control,
                &clocks,
            ),
            chip_select: adc_cs,
        },
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
    Menu,
    Shutdown,
}

#[embassy_executor::task]
async fn main_task(mut resources: Resources) {
    // If the device is awake, the display should be enabled.
    let mut display = resources.display.enable().await.unwrap();

    let mut state = AppState::Initialize;

    loop {
        state = match state {
            AppState::Initialize => initialize(&mut display, &mut resources.frontend).await,
            AppState::Measure => measure(&mut display, &mut resources.frontend).await,
            AppState::Menu => menu(&mut display, &mut resources.frontend).await,
            AppState::Shutdown => {
                display.shut_down();

                let (_, _, _, touch) = resources.frontend.split();
                enter_deep_sleep(touch);
            }
        };
    }
}

const MIN_FRAME_TIME: Duration = Duration::from_millis(10);

async fn initialize(
    display: &mut display::PoweredDisplay<'_, DisplayInterface<'_>, DisplayReset>,
    frontend: &mut Frontend<AdcSpi<'_>, AdcDrdy, AdcReset, TouchDetect>,
) -> AppState {
    const INIT_TIME: Duration = Duration::from_secs(20);
    const MENU_THRESHOLD: Duration = Duration::from_secs(10);

    let entered = Instant::now();
    let mut ticker = Ticker::every(MIN_FRAME_TIME);
    while let elapsed = entered.elapsed() && elapsed <= INIT_TIME {
        display_init_screen(display, elapsed);

        display.flush().await.unwrap();

        if !frontend.is_touched() {
            return if elapsed > MENU_THRESHOLD {
                AppState::Menu
            } else {
                AppState::Shutdown
            };
        }

        ticker.next().await;
    }

    AppState::Measure
}

fn display_init_screen(
    display: &mut impl DrawTarget<Color = BinaryColor, Error: Debug>,
    elapsed: Duration,
) {
    display.clear(BinaryColor::Off).unwrap();

    todo!("Based on elapsed, display a message and a progress bar")
}

async fn measure(
    display: &mut display::PoweredDisplay<'_, DisplayInterface<'_>, DisplayReset>,
    frontend: &mut Frontend<AdcSpi<'_>, AdcDrdy, AdcReset, TouchDetect>,
) -> AppState {
    // let frontend = frontend.enable_async().await.unwrap();

    todo!()
}

async fn menu(
    display: &mut display::PoweredDisplay<'_, DisplayInterface<'_>, DisplayReset>,
    frontend: &mut Frontend<AdcSpi<'_>, AdcDrdy, AdcReset, TouchDetect>,
) -> AppState {
    todo!()
}

fn enter_deep_sleep(wakeup_pin: TouchDetect) -> ! {
    let rtc = unsafe { &*pac::RTC_CNTL::PTR };

    todo!()
}
