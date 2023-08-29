#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use core::{
    future::Future,
    sync::atomic::{AtomicPtr, Ordering},
};
use embassy_executor::{
    raw::{AvailableTask, TaskStorage},
    SpawnToken,
};

/// Raw storage that can hold up to N tasks of the same type.
///
/// The tasks are heap-allocated on demand, so this pool can be used to save memory if the
/// application is not expected to run some combination of tasks in the same power cycle.
///
/// This is essentially a `[Box<TaskStorage<F>>; N]`.
pub struct AllocTaskPool<F: Future + 'static, const N: usize> {
    pool: [AtomicPtr<TaskStorage<F>>; N],
}

impl<F: Future + 'static, const N: usize> AllocTaskPool<F, N> {
    const NULL_PTR: AtomicPtr<TaskStorage<F>> = AtomicPtr::new(core::ptr::null_mut());

    /// Create a new AllocTaskPool, with all tasks in non-spawned state.
    pub const fn new() -> Self {
        Self {
            pool: [Self::NULL_PTR; N],
        }
    }

    fn allocate(&'static self) -> Option<AvailableTask<F>> {
        if let Some(task) = self
            .pool
            .iter()
            .filter_map(|ptr| unsafe { ptr.load(Ordering::SeqCst).as_ref() })
            .find_map(AvailableTask::claim)
        {
            return Some(task);
        }

        for ptr in self.pool.iter() {
            if ptr
                .compare_exchange(
                    core::ptr::null_mut(),
                    core::mem::align_of::<TaskStorage<F>>() as *mut _,
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                )
                .is_ok()
            {
                let storage = Box::leak(Box::new(TaskStorage::new()));

                ptr.store(storage as *mut _, Ordering::SeqCst);

                let reference = unsafe { ptr.load(Ordering::SeqCst).as_ref() };
                return reference.and_then(AvailableTask::claim);
            }
        }

        None
    }

    /// Try to spawn a task in the pool.
    ///
    /// See [`TaskStorage::spawn()`] for details.
    ///
    /// This will loop over the pool and spawn the task in the first storage that
    /// is currently free. If none is free, a "poisoned" SpawnToken is returned,
    /// which will cause [`Spawner::spawn()`](super::Spawner::spawn) to return the error.
    #[allow(unused)]
    pub fn spawn(&'static self, future: impl FnOnce() -> F) -> SpawnToken<impl Sized> {
        match self.allocate() {
            Some(task) => task.initialize(future),
            None => SpawnToken::new_failed(),
        }
    }

    /// Like spawn(), but allows the task to be send-spawned if the args are Send even if
    /// the future is !Send.
    ///
    /// Not covered by semver guarantees. DO NOT call this directly. Intended to be used
    /// by the Embassy macros ONLY.
    ///
    /// SAFETY: `future` must be a closure of the form `move || my_async_fn(args)`, where `my_async_fn`
    /// is an `async fn`, NOT a hand-written `Future`.
    #[doc(hidden)]
    pub unsafe fn _spawn_async_fn<FutFn>(&'static self, future: FutFn) -> SpawnToken<impl Sized>
    where
        FutFn: FnOnce() -> F,
    {
        // When send-spawning a task, we construct the future in this thread, and effectively
        // "send" it to the executor thread by enqueuing it in its queue. Therefore, in theory,
        // send-spawning should require the future `F` to be `Send`.
        //
        // The problem is this is more restrictive than needed. Once the future is executing,
        // it is never sent to another thread. It is only sent when spawning. It should be
        // enough for the task's arguments to be Send. (and in practice it's super easy to
        // accidentally make your futures !Send, for example by holding an `Rc` or a `&RefCell` across an `.await`.)
        //
        // We can do it by sending the task args and constructing the future in the executor thread
        // on first poll. However, this cannot be done in-place, so it'll waste stack space for a copy
        // of the args.
        //
        // Luckily, an `async fn` future contains just the args when freshly constructed. So, if the
        // args are Send, it's OK to send a !Send future, as long as we do it before first polling it.
        //
        // (Note: this is how the generators are implemented today, it's not officially guaranteed yet,
        // but it's possible it'll be guaranteed in the future. See zulip thread:
        // https://rust-lang.zulipchat.com/#narrow/stream/187312-wg-async/topic/.22only.20before.20poll.22.20Send.20futures )
        //
        // The `FutFn` captures all the args, so if it's Send, the task can be send-spawned.
        // This is why we return `SpawnToken<FutFn>` below.
        //
        // This ONLY holds for `async fn` futures. The other `spawn` methods can be called directly
        // by the user, with arbitrary hand-implemented futures. This is why these return `SpawnToken<F>`.

        match self.allocate() {
            Some(task) => task.__initialize_async_fn::<FutFn>(future),
            None => SpawnToken::new_failed(),
        }
    }
}
