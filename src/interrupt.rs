use core::marker::PhantomData;
use core::mem::MaybeUninit;
use core::ptr;
use core::sync::atomic::Ordering;
use core::{cell::UnsafeCell, sync::atomic::AtomicBool};

use embassy_executor::raw::{self, Pender};

type SystemPeripheral = peripherals::SYSTEM;

use crate::board::hal::{interrupt, peripherals};

pub trait SwInterrupt {
    // All cores should enable the interrupt
    fn enable();
    fn pend(ctxt: *mut ());
    fn clear();
}

pub struct SwInterrupt0;

impl SwInterrupt for SwInterrupt0 {
    fn enable() {
        interrupt::enable(
            peripherals::Interrupt::FROM_CPU_INTR0,
            interrupt::Priority::Priority3,
        )
        .unwrap();
    }

    fn pend(_ctxt: *mut ()) {
        let system = unsafe { &*SystemPeripheral::PTR };
        system
            .cpu_intr_from_cpu_0
            .write(|w| w.cpu_intr_from_cpu_0().bit(true));
    }

    fn clear() {
        let system = unsafe { &*SystemPeripheral::PTR };
        system
            .cpu_intr_from_cpu_0
            .write(|w| w.cpu_intr_from_cpu_0().bit(false));
    }
}

/// Interrupt mode executor.
///
/// This executor runs tasks in interrupt mode. The interrupt handler is set up
/// to poll tasks, and when a task is woken the interrupt is pended from software.
pub struct InterruptExecutor<SWI>
where
    SWI: SwInterrupt,
{
    started: AtomicBool,
    executor: UnsafeCell<MaybeUninit<raw::Executor>>,
    _interrupt: PhantomData<SWI>,
}

unsafe impl<SWI: SwInterrupt> Send for InterruptExecutor<SWI> {}
unsafe impl<SWI: SwInterrupt> Sync for InterruptExecutor<SWI> {}

impl<SWI> InterruptExecutor<SWI>
where
    SWI: SwInterrupt,
{
    /// Create a new, not started `RawInterruptExecutor`.
    #[inline]
    pub const fn new() -> Self {
        Self {
            started: AtomicBool::new(false),
            executor: UnsafeCell::new(MaybeUninit::uninit()),
            _interrupt: PhantomData,
        }
    }

    /// Executor interrupt callback.
    ///
    /// # Safety
    ///
    /// You MUST call this from the interrupt handler, and from nowhere else.
    pub unsafe fn on_interrupt(&'static self) {
        SWI::clear();
        let executor = unsafe { (&*self.executor.get()).assume_init_ref() };
        executor.poll();
    }

    /// Start the executor.
    ///
    /// This initializes the executor, enables the interrupt, and returns.
    /// The executor keeps running in the background through the interrupt.
    ///
    /// This returns a [`SendSpawner`] you can use to spawn tasks on it. A [`SendSpawner`]
    /// is returned instead of a [`Spawner`](embassy_executor::Spawner) because the executor effectively runs in a
    /// different "thread" (the interrupt), so spawning tasks on it is effectively
    /// sending them.
    ///
    /// To obtain a [`Spawner`](embassy_executor::Spawner) for this executor, use [`Spawner::for_current_executor()`](embassy_executor::Spawner::for_current_executor()) from
    /// a task running in it.
    ///
    /// # Interrupt requirements
    ///
    /// You must write the interrupt handler yourself, and make it call [`on_interrupt()`](Self::on_interrupt).
    ///
    /// This method already enables (unmasks) the interrupt, you must NOT do it yourself.
    ///
    /// You must set the interrupt priority before calling this method. You MUST NOT
    /// do it after.
    ///
    pub fn start(&'static self) -> embassy_executor::SendSpawner {
        if self
            .started
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            panic!("InterruptExecutor::start() called multiple times on the same executor.");
        }

        unsafe {
            (&mut *self.executor.get())
                .as_mut_ptr()
                .write(raw::Executor::new(Pender::new_from_callback(
                    SWI::pend,
                    ptr::null_mut(),
                )))
        }

        let executor = unsafe { (&*self.executor.get()).assume_init_ref() };
        executor.spawner().make_send()
    }
}
