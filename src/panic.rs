//! The hooks of the `esp-backtrace` panic handler. It logs the panic and a backtrace, and the
//! hooks reset the device, so that the next boot can save the logs.

use core::sync::atomic::{AtomicBool, Ordering};

use esp_backtrace as _;
use esp_hal::{debugger::debugger_connected, system};
use semihosting::process;

use crate::board::logging;

static PANICKED: AtomicBool = AtomicBool::new(false);

fn halt() -> ! {
    // Without a debugger, the semihosting call is an exception.
    if debugger_connected() {
        process::abort();
    }

    system::software_reset()
}

#[no_mangle]
fn custom_pre_backtrace() {
    // SAFETY: the panic handler never leaves the critical section.
    let _ = unsafe { critical_section::acquire() };

    // A panic inside the handler comes back here.
    if PANICKED.swap(true, Ordering::Relaxed) {
        halt();
    }

    // SAFETY: the critical section above never ends.
    unsafe { logging::panic_started() };
}

#[no_mangle]
fn custom_halt() -> ! {
    // SAFETY: `custom_pre_backtrace` took a critical section that never ends.
    unsafe { logging::panic_logged() };

    halt()
}
