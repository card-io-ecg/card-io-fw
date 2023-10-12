use core::{cell::UnsafeCell, future::Future};

use alloc::sync::Arc;
use embassy_futures::select::{select, Either};
use embassy_sync::{blocking_mutex::raw::NoopRawMutex, signal::Signal};

#[non_exhaustive]
pub struct Aborted {}
struct Inner<R: Send, D: Send = ()> {
    /// Used to signal the controlled task to stop.
    token: Signal<NoopRawMutex, ()>,

    /// Used to indicate that the controlled task has exited, and may include a return value.
    exited: Signal<NoopRawMutex, Result<R, Aborted>>,

    resources: UnsafeCell<D>,
}

unsafe impl<R: Send, D: Send> Send for Inner<R, D> {}
unsafe impl<R: Send, D: Send> Sync for Inner<R, D> {}

impl<R: Send, D: Send> Inner<R, D> {
    /// Creates a new signal pair.
    const fn new(resources: D) -> Self {
        Self {
            token: Signal::new(),
            exited: Signal::new(),
            resources: UnsafeCell::new(resources),
        }
    }

    /// Stops the controlled task, and returns its return value.
    async fn stop_from_outside(&self) -> Result<R, Aborted> {
        // Signal the task to stop.
        self.token.signal(());

        // Wait for the task to exit.
        self.exited.wait().await
    }

    /// Returns whether the controlled task has exited.
    fn has_exited(&self) -> bool {
        self.exited.signaled()
    }

    /// Runs a cancellable task. The task ends when either the future completes, or the task is
    /// cancelled.
    async fn run_cancellable<'a, F>(&'a self, f: impl FnOnce(&'a mut D) -> F)
    where
        F: Future<Output = R> + 'a,
    {
        self.token.reset();
        self.exited.reset();

        let resources = unsafe { unwrap!(self.resources.get().as_mut()) };

        let result = match select(f(resources), self.token.wait()).await {
            Either::First(result) => Ok(result),
            Either::Second(_) => Err(Aborted {}),
        };
        self.exited.signal(result)
    }
}

pub struct TaskController<R: Send, D: Send = ()> {
    inner: Arc<Inner<R, D>>,
}

impl<R: Send> TaskController<R, ()> {
    /// Creates a new signal pair.
    pub fn new() -> Self {
        Self::from_resources(())
    }
}

impl<R: Send, D: Send> TaskController<R, D> {
    /// Creates a new signal pair.
    pub fn from_resources(resources: D) -> Self {
        Self {
            inner: Arc::new(Inner::new(resources)),
        }
    }

    /// Stops the controlled task, and returns its return value.
    pub async fn stop(&self) -> Result<R, Aborted> {
        self.inner.stop_from_outside().await
    }

    /// Returns whether the controlled task has exited.
    pub fn has_exited(&self) -> bool {
        self.inner.has_exited()
    }

    pub fn token(&self) -> TaskControlToken<R, D> {
        // We only allow a single token that can be passed to a task.
        debug_assert_eq!(Arc::strong_count(&self.inner), 1);
        TaskControlToken {
            inner: self.inner.clone(),
        }
    }

    pub fn unwrap(self) -> D {
        let inner = self.inner.clone();
        core::mem::drop(self);
        unwrap!(Arc::try_unwrap(inner).ok()).resources.into_inner()
    }
}

impl<R: Send, D: Send> Drop for TaskController<R, D> {
    fn drop(&mut self) {
        if Arc::strong_count(&self.inner) > 1 {
            self.inner.token.signal(());
        }
    }
}

pub struct TaskControlToken<R: Send, D: Send = ()> {
    inner: Arc<Inner<R, D>>,
}

impl<R: Send, D: Send> TaskControlToken<R, D> {
    /// Runs a cancellable task. The task ends when either the future completes, or the task is
    /// cancelled.
    pub async fn run_cancellable<'a, F>(&'a mut self, f: impl FnOnce(&'a mut D) -> F)
    where
        F: Future<Output = R> + 'a,
    {
        self.inner.run_cancellable(f).await
    }
}
