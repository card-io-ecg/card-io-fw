use core::ops::Range;

use esp_hal::{
    assist_debug::DebugAssist,
    handler, interrupt,
    peripherals::{self, ASSIST_DEBUG},
    system::Cpu,
};

pub struct StackMonitor {
    assist: DebugAssist<'static>,
}

fn conjure() -> DebugAssist<'static> {
    let peripheral = unsafe { ASSIST_DEBUG::steal() };
    DebugAssist::new(peripheral)
}

impl StackMonitor {
    /// Enable stack overflow detection for the given memory region, for the current CPU core.
    /// The stack grows from high address (top) to low address (bottom). We place a 4-byte canary at
    /// the end of the stack, and watch for reads from and writes to it.
    ///
    /// Note that this is not perfect as code may simply access memory below the canary without
    /// accessing the canary prior to that. However, this is a good enough approximation for our
    /// purposes.
    pub fn protect(stack: Range<usize>) -> Self {
        let (bottom, top) = if stack.start < stack.end {
            (stack.start, stack.end)
        } else {
            (stack.end, stack.start)
        };

        info!(
            "StackMonitor::protect({:?}, {})",
            top as *const u32,
            top - bottom
        );
        let peripheral = unsafe { ASSIST_DEBUG::steal() };
        let mut assist = DebugAssist::new(peripheral);

        assist.set_interrupt_handler(interrupt_handler);

        const CANARY_UNITS: u32 = 1;
        const CANARY_GRANULARITY: u32 = 16;

        // We watch writes to the last word in the stack.
        match Cpu::current() {
            Cpu::ProCpu => assist.enable_region0_monitor(
                bottom as u32 + CANARY_GRANULARITY,
                bottom as u32 + CANARY_GRANULARITY + CANARY_UNITS * CANARY_GRANULARITY,
                true,
                true,
            ),
            #[cfg(feature = "esp32s3")]
            Cpu::AppCpu => assist.enable_core1_region0_monitor(
                bottom as u32 + CANARY_GRANULARITY,
                bottom as u32 + CANARY_GRANULARITY + CANARY_UNITS * CANARY_GRANULARITY,
                true,
                true,
            ),
        }

        unwrap!(interrupt::enable(
            peripherals::Interrupt::ASSIST_DEBUG,
            interrupt::Priority::Priority3,
        ));

        Self { assist }
    }
}

impl Drop for StackMonitor {
    fn drop(&mut self) {
        match Cpu::current() {
            Cpu::ProCpu => self.assist.disable_region0_monitor(),
            #[cfg(feature = "esp32s3")]
            Cpu::AppCpu => self.assist.disable_core1_region0_monitor(),
        }
    }
}

#[handler(priority = esp_hal::interrupt::Priority::max())]
fn interrupt_handler() {
    let mut da = conjure();
    let cpu = Cpu::current();

    let pc;
    let is_overflow;

    match cpu {
        Cpu::ProCpu => {
            is_overflow = da.is_region0_monitor_interrupt_set();
            pc = da.region_monitor_pc();
            da.clear_region0_monitor_interrupt();
        }
        #[cfg(feature = "esp32s3")]
        Cpu::AppCpu => {
            is_overflow = da.is_core1_region0_monitor_interrupt_set();
            pc = da.core1_region_monitor_pc();
            da.clear_core1_region0_monitor_interrupt();
        }
    }

    if is_overflow {
        panic!("Core {:?} stack overflow detected - PC = {:#X}", cpu, pc);
    }
}
