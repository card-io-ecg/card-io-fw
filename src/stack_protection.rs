use core::ops::Range;

use crate::board::hal::{
    assist_debug::DebugAssist,
    get_core, interrupt,
    peripherals::{self, ASSIST_DEBUG, SYSTEM},
    prelude::*,
    Cpu,
};

pub struct StackMonitor {
    assist: DebugAssist<'static>,
}

fn conjure() -> DebugAssist<'static> {
    let mut system = unsafe { SYSTEM::steal() }.split();

    let peripheral = unsafe { ASSIST_DEBUG::steal() };
    DebugAssist::new(peripheral, &mut system.peripheral_clock_control)
}

impl StackMonitor {
    pub fn protect(stack: Range<u32>) -> Self {
        log::info!(
            "StackMonitor::protect({:#x}, {})",
            stack.start,
            stack.end - stack.start
        );
        let mut assist = conjure();

        // We watch writes to the last word in the stack.
        match get_core() {
            Cpu::ProCpu => assist.enable_region0_monitor(stack.start, stack.start + 4, true, true),
            Cpu::AppCpu => {
                assist.enable_core1_region0_monitor(stack.start, stack.start + 4, true, true)
            }
        }

        interrupt::enable(
            peripherals::Interrupt::ASSIST_DEBUG,
            interrupt::Priority::Priority3,
        )
        .unwrap();

        Self { assist }
    }
}

impl Drop for StackMonitor {
    fn drop(&mut self) {
        match get_core() {
            Cpu::ProCpu => self.assist.disable_region0_monitor(),
            Cpu::AppCpu => self.assist.disable_core1_region0_monitor(),
        }
    }
}

#[interrupt]
fn ASSIST_DEBUG() {
    let mut da = conjure();
    let cpu = get_core();

    let pc;
    let is_overflow;

    match cpu {
        Cpu::ProCpu => {
            is_overflow = da.is_region0_monitor_interrupt_set();
            pc = da.get_region_monitor_pc();
            da.clear_region0_monitor_interrupt();
        }
        Cpu::AppCpu => {
            is_overflow = da.is_core1_region0_monitor_interrupt_set();
            pc = da.get_core1_region_monitor_pc();
            da.clear_core1_region0_monitor_interrupt();
        }
    }

    if is_overflow {
        panic!("Core {cpu:?} stack overflow detected - PC = 0x{pc:x}");
    }
}
