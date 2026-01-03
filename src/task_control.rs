//! Alloc-based task control and shared resource management
//!
//! This module implements a way to share data with tasks, and to allow cancelling them.
//!
//! [`TaskController`] wraps a set of resources that are passed to the spawned task, and are
//! passed back to the spawner when the task finishes, and can be retrieved by [`TaskController::unwrap`].
//! It is useful when the short-lived task needs exclusive access to hardware.
//!
//! [`TaskController`] also provides a way to cancel tasks and observe if they are still running.
//!
//! This is implemented with an [`Arc`][alloc::sync::Arc] so that the resources and signals don't
//! need to be stored in a `'static`.

// TODO: task_control should be split in two:
// - The actual signal pair to control tasks
// - The shared resource management part

use core::{cell::UnsafeCell, future::Future};

use alloc::sync::Arc;
use embassy_futures::select::{select, Either};
use embassy_sync::signal::Signal;
use esp_sync::RawMutex;

/// The return value of the task, when cancelled.
#[non_exhaustive]
pub struct Aborted {}

/// Implementation details.
struct Inner<R: Send, D: Send = ()> {
    /// Used to signal the controlled task to stop.
    token: Signal<RawMutex, ()>,

    /// Used to indicate that the controlled task has exited, and may include a return value.
    exited: Signal<RawMutex, Result<R, Aborted>>,

    /// Data provided by the task that starts the controlled task. Accessed by `run_cancellable`.
    /// While the task is running, the task is considered to be the owner of `D`. Once the task
    /// has stopped (either it finished or it was cancelled), the resources are sent back to the
    /// task that created the [`TaskController`].
    resources: UnsafeCell<D>,
}

// TODO: When the resource/control parts are split apart,
// the control struct shouldn't need an unsafe impl.
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
    ///
    /// # Safety
    ///
    /// The caller must ensure this function is not called reentrantly.
    async unsafe fn run_cancellable<'a, F>(&'a self, f: impl FnOnce(&'a mut D) -> F)
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
    /// Creates a new signal pair with a set of resources.
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
    // FIXME: `TaskControlToken::run_cancellable` can be called multiple times, so
    // this fn returning `true` doesn't guarantee that the async task itself has stopped.
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
        unsafe {
            // Safety: this is the only call site of `Inner::run_cancellable` and
            // `Self::run_cancellable` takes `&mut self`.
            self.inner.run_cancellable(f).await
        }
    }
}
