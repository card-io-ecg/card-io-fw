use core::future::Future;

use embassy_futures::select::{select, Either};
use embassy_time::{Duration, Instant, Timer};

pub struct Timeout {
    start: Instant,
    duration: Duration,
}

impl Timeout {
    pub fn new(duration: Duration) -> Self {
        Self::new_with_start(duration, Instant::now())
    }

    pub fn new_with_start(duration: Duration, start: Instant) -> Self {
        Self { start, duration }
    }

    pub fn reset(&mut self) {
        self.start = Instant::now();
    }

    pub fn is_elapsed(&self) -> bool {
        self.elapsed() >= self.duration
    }

    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }

    pub fn remaining(&self) -> Duration {
        self.duration - self.elapsed()
    }

    pub async fn with<R>(duration: Duration, f: impl Future<Output = R>) -> Option<R> {
        let result = select(f, Timer::after(duration)).await;

        match result {
            Either::First(result) => Some(result),
            Either::Second(_) => None,
        }
    }
}
