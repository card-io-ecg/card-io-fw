//! The `defmt` logger. Each frame goes to the transport that a feature selects, and to a ring
//! that survives a reset. After an unexpected boot the ring goes to the store.

use alloc::vec::Vec;

use critical_section::RestoreState;
use defmt::Encoder;
use esp_hal::{debugger::debugger_connected, rtc_cntl::SocResetReason, system, Persistable};
use log_ring::{encode_record, LogRing, Pending, RecordInfo, MAX_VERSION_LEN, RECORD_HEADER};

use crate::board::storage::{FileSystem, StorageError};

#[cfg_attr(feature = "rtt", path = "logging/rtt.rs")]
#[cfg_attr(feature = "esp-println", path = "logging/println.rs")]
mod transport;

use transport::Transport;

const RING_BYTES: usize = 2048;
const RECORD_CAPACITY: usize = RECORD_HEADER + 2 * MAX_VERSION_LEN + RING_BYTES;

type Ring = LogRing<RING_BYTES>;

#[repr(transparent)]
struct Retained(Ring);

// SAFETY: `LogRing` holds integers and byte arrays only, and each bit pattern is a valid value.
unsafe impl Persistable for Retained {}

#[esp_hal::ram(unstable(rtc_fast, persistent))]
static mut RING: Retained = Retained(Ring::CLEARED);

struct State {
    transport: Transport,
    encoder: Encoder,
    /// The count of log calls that are in progress. Only the first one writes.
    depth: u8,
    restore: RestoreState,
    /// False before `init`, and while a record waits for the store.
    ring_open: bool,
    pending: Option<Pending>,
}

static mut STATE: State = State {
    transport: Transport::new(),
    encoder: Encoder::new(),
    depth: 0,
    restore: RestoreState::invalid(),
    ring_open: false,
    pending: None,
};

/// # Safety
///
/// The caller is in a critical section and holds no other reference from this function.
unsafe fn parts() -> (&'static mut State, &'static mut Ring) {
    unsafe { (&mut *(&raw mut STATE), &mut (*(&raw mut RING)).0) }
}

/// `f` must not log.
fn with<R>(f: impl FnOnce(&mut State, &mut Ring) -> R) -> R {
    critical_section::with(|_| {
        // SAFETY: the critical section excludes the logger and other callers.
        let (state, ring) = unsafe { parts() };
        f(state, ring)
    })
}

impl State {
    fn sinks<'a>(
        transport: &'a mut Transport,
        mut ring: Option<&'a mut Ring>,
    ) -> impl FnMut(&[u8]) + 'a {
        move |bytes| {
            transport.write(bytes);
            if let Some(ring) = ring.as_mut() {
                for byte in bytes {
                    ring.push(*byte);
                }
            }
        }
    }

    fn start_frame(&mut self, ring: &mut Ring) {
        self.transport.start_frame();

        let ring = self.ring_open.then_some(ring);
        self.encoder
            .start_frame(Self::sinks(&mut self.transport, ring));
    }

    fn write(&mut self, bytes: &[u8], ring: &mut Ring) {
        let ring = self.ring_open.then_some(ring);
        self.encoder
            .write(bytes, Self::sinks(&mut self.transport, ring));
    }

    fn end_frame(&mut self, ring: &mut Ring) {
        let ring = self.ring_open.then_some(ring);
        self.encoder
            .end_frame(Self::sinks(&mut self.transport, ring));

        self.transport.end_frame();
    }
}

#[defmt::global_logger]
struct Logger;

unsafe impl defmt::Logger for Logger {
    fn acquire() {
        // SAFETY: `release` gives the state back, in reverse order for a nested call.
        let restore = unsafe { critical_section::acquire() };
        // SAFETY: we are in a critical section.
        let (state, ring) = unsafe { parts() };

        state.depth += 1;
        if state.depth > 1 {
            // SAFETY: this is the state that the `acquire` above returned.
            unsafe { critical_section::release(restore) };
            return;
        }

        state.restore = restore;
        state.start_frame(ring);
    }

    unsafe fn flush() {}

    unsafe fn release() {
        // SAFETY: `acquire` left us in a critical section.
        let (state, ring) = unsafe { parts() };

        state.depth -= 1;
        if state.depth > 0 {
            return;
        }

        state.end_frame(ring);
        // SAFETY: this is the state that the first `acquire` returned.
        unsafe { critical_section::release(state.restore) };
    }

    unsafe fn write(bytes: &[u8]) {
        // SAFETY: `acquire` left us in a critical section.
        let (state, ring) = unsafe { parts() };

        if state.depth == 1 {
            state.write(bytes, ring);
        }
    }
}

fn deep_sleep_wake() -> bool {
    system::reset_reason() == Some(SocResetReason::CoreDeepSleep)
}

/// Starts the transport and decides what the ring of the last boot is worth. Logs before this
/// call do not reach the ring.
pub fn init() {
    with(|state, ring| {
        state.transport.init();
        state.pending = ring.boot(deep_sleep_wake());
        state.ring_open = state.pending.is_none();
    });
}

/// Saves the record of an unexpected boot, unless a debugger is connected. The ring takes no
/// logs from `init` until this call ends.
pub async fn save(storage: Option<&mut FileSystem>) {
    let Some(pending) = with(|state, _| state.pending.take()) else {
        return;
    };

    match storage {
        // A flash or a reset through the probe is no crash.
        _ if debugger_connected() => debug!("Debugger connected. Log record dropped"),
        Some(storage) => {
            if let Err(e) = store_record(storage, pending).await {
                warn!("Failed to save the log record: {:?}", e);
            }
        }
        None => warn!("Storage unavailable. Log record dropped"),
    }

    with(|state, ring| {
        ring.saved(pending);
        state.ring_open = true;
    });
}

async fn store_record(storage: &mut FileSystem, pending: Pending) -> Result<(), StorageError> {
    let sequence = storage.next_log_sequence().await?;

    let mut record = Vec::new();
    record
        .try_reserve_exact(RECORD_CAPACITY)
        .map_err(|_| StorageError::OutOfMemory)?;
    record.resize(RECORD_CAPACITY, 0);

    let info = RecordInfo {
        pending,
        reset_reason: system::reset_reason().map_or(0, |reason| reason as u8),
        sequence,
        firmware_version: env!("COMMIT_HASH"),
        hardware_version: env!("HW_VERSION"),
    };
    let len = with(|_, ring| encode_record(ring, &info, &mut record));
    // `record` holds the largest record, and `build.rs` makes versions that fit.
    let len = unwrap!(len);

    storage.store_log_record(sequence, &record[..len]).await
}

/// The boot works. The next unexpected boot starts a new streak of records.
pub fn mark_healthy() {
    with(|_, ring| ring.mark_healthy());
}

/// Makes the logger ready for the panic message.
///
/// # Safety
///
/// The caller is in a critical section that it never leaves.
pub unsafe fn panic_started() {
    // SAFETY: the caller is in a critical section.
    let (state, ring) = unsafe { parts() };

    // The panic can come from inside a log call. That call never ends its frame.
    if state.depth > 0 {
        state.depth = 0;
        state.end_frame(ring);
    }

    // A record that still waits for the store is lost with this boot.
    state.pending = None;
    state.ring_open = true;
}

/// Marks the ring, so that the record of the next boot names the panic.
///
/// # Safety
///
/// The caller is in a critical section that it never leaves.
pub unsafe fn panic_logged() {
    // SAFETY: the caller is in a critical section.
    let (_, ring) = unsafe { parts() };
    ring.panicked();
}
