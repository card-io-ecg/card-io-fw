use embassy_time::{Duration, Instant};

pub struct Timeout {
    start: Instant,
    duration: Duration,
}

impl Timeout {
    pub fn new(duration: Duration) -> Self {
        Self {
            start: Instant::now(),
            duration,
        }
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
}
