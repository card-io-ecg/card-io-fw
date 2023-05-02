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
    embassy,
    peripherals::Peripherals,
    prelude::*,
    timer::TimerGroup,
    Rtc,
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
