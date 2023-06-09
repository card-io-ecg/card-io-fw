use core::future::Future;

use embassy_futures::select::{select, Either};
use embassy_sync::{blocking_mutex::raw::NoopRawMutex, signal::Signal};

#[non_exhaustive]
pub struct Aborted {}

pub struct TaskController<R: Send> {
    /// Used to signal the controlled task to stop.
    token: Signal<NoopRawMutex, ()>,

    /// Used to indicate that the controlled task has exited, and may include a return value.
    exited: Signal<NoopRawMutex, Result<R, Aborted>>,
}

impl<R: Send> TaskController<R> {
    pub const DEFAULT: Self = Self::new();

    /// Creates a new signal pair.
    pub const fn new() -> Self {
        Self {
            token: Signal::new(),
            exited: Signal::new(),
        }
    }

    /// Stops the controlled task, and returns its return value.
    pub async fn stop_from_outside(&self) -> Result<R, Aborted> {
        // Signal the task to stop.
        self.token.signal(());

        // Wait for the task to exit.
        self.exited.wait().await
    }

    /// Returns whether the controlled task has exited.
    pub fn has_exited(&self) -> bool {
        self.exited.signaled()
    }

    /// Runs a cancellable task. The task ends when either the future completes, or the task is
    /// cancelled.
    pub async fn run_cancellable(&self, future: impl Future<Output = R>) {
        self.token.reset();
        self.exited.reset();
        let result = match select(future, self.token.wait()).await {
            Either::First(result) => Ok(result),
            Either::Second(_) => Err(Aborted {}),
        };
        self.exited.signal(result)
    }
}
