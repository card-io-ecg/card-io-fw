#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]

extern crate alloc;

use embassy_executor::{Executor, _export::StaticCell};
use embassy_time::{Duration, Ticker};
use esp_backtrace as _;

#[cfg(feature = "esp32s2")]
pub use esp32s2_hal as hal;

#[cfg(feature = "esp32s3")]
pub use esp32s3_hal as hal;

use esp_println::logger::init_logger;

use hal::{
    clock::{ClockControl, CpuClock},
    dma::DmaPriority,
    embassy,
    gdma::Gdma,
    peripherals::Peripherals,
    prelude::*,
    spi::SpiMode,
    timer::TimerGroup,
    Rtc, Spi, IO,
};

mod heap;

use crate::heap::init_heap;

static EXECUTOR: StaticCell<Executor> = StaticCell::new();

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

    let _display_reset = io.pins.gpio9.into_push_pull_output();
    let _display_dc = io.pins.gpio13.into_push_pull_output();

    let mut display_cs = io.pins.gpio10;
    let display_sclk = io.pins.gpio12;
    let display_mosi = io.pins.gpio11;

    let display_spi = peripherals.SPI2;

    display_cs
        .set_to_push_pull_output()
        .connect_peripheral_to_output(display_spi.cs_signal());

    let mut display_spi = Spi::new_no_cs_no_miso(
        display_spi,
        display_sclk,
        display_mosi,
        100u32.kHz(),
        SpiMode::Mode0,
        &mut system.peripheral_clock_control,
        &clocks,
    )
    .with_dma(display_dma_channel.configure(
        false,
        &mut [0u32; 8 * 3],
        &mut [0u32; 8 * 3],
        DmaPriority::Priority0,
    ));

    let executor = EXECUTOR.init(Executor::new());
    executor.run(|spawner| {
        spawner.spawn(ticker_task()).ok();
    });
}

#[embassy_executor::task]
async fn ticker_task() {
    let mut ticker = Ticker::every(Duration::from_millis(500));
    loop {
        ticker.next().await;
        log::info!("Tick");
        ticker.next().await;
        log::info!("Tock");
    }
}
