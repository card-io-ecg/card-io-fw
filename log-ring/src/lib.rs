//! A log ring that survives a reset, and the record that holds its bytes in the store.

#![no_std]

mod record;

pub use record::{
    decode_next, encode_record, record_key, waiting, RecordInfo, MAX_VERSION_LEN, NEXT_KEY,
    RECORD_HEADER,
};

// The bytes "LRG1".
const MAGIC: u32 = 0x3147_524C;

const WRITE_WEIGHT: u32 = 0x9E37_79B1;
const STATE_WEIGHT: u32 = 0x85EB_CA6B;

const PANIC: u8 = 1 << 0;
const PLANNED: u8 = 1 << 1;

/// One streak of unexpected boots saves this many records.
const STREAK_RECORDS: u8 = 4;

/// The newest logs, as encoded frames. The ring overwrites its oldest byte.
///
/// Each bit pattern is a valid value, so the ring can live in memory that the startup code does
/// not initialise. Call [`LogRing::boot`] before any other method.
#[repr(C)]
pub struct LogRing<const N: usize> {
    magic: u32,
    write: u32,
    // check = Σ (2·i + 1) · data[i] + WRITE_WEIGHT · write + STATE_WEIGHT · (flags | streak << 8)
    check: u32,
    flags: u8,
    streak: u8,
    reserved: [u8; 2],
    data: [u8; N],
}

/// A record that waits for the store. The ring must not change until [`LogRing::saved`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Pending {
    /// The ring was not intact.
    pub suspect: bool,
    pub panic: bool,
}

impl<const N: usize> LogRing<N> {
    pub const CLEARED: Self = Self {
        magic: MAGIC,
        write: 0,
        check: 0,
        flags: 0,
        streak: 0,
        reserved: [0; 2],
        data: [0; N],
    };

    fn state_term(write: u32, flags: u8, streak: u8) -> u32 {
        let state = u32::from(flags) | u32::from(streak) << 8;
        WRITE_WEIGHT
            .wrapping_mul(write)
            .wrapping_add(STATE_WEIGHT.wrapping_mul(state))
    }

    fn slot_weight(slot: usize) -> u32 {
        2 * slot as u32 + 1
    }

    fn checksum(&self) -> u32 {
        let data = self
            .data
            .iter()
            .enumerate()
            .fold(0u32, |sum, (slot, byte)| {
                sum.wrapping_add(Self::slot_weight(slot).wrapping_mul(u32::from(*byte)))
            });

        data.wrapping_add(Self::state_term(self.write, self.flags, self.streak))
    }

    fn usable(&self) -> bool {
        self.magic == MAGIC && (self.write as usize) < N && self.reserved == [0; 2]
    }

    fn intact(&self) -> bool {
        self.usable() && self.check == self.checksum()
    }

    fn clear(&mut self, streak: u8) {
        *self = Self::CLEARED;
        self.streak = streak;
        self.check = Self::state_term(0, 0, streak);
    }

    fn set_state(&mut self, flags: u8, streak: u8) {
        let old = Self::state_term(self.write, self.flags, self.streak);
        let new = Self::state_term(self.write, flags, streak);

        self.flags = flags;
        self.streak = streak;
        self.check = self.check.wrapping_add(new.wrapping_sub(old));
    }

    /// Decides what the ring of the last boot is worth. `deep_sleep_wake` tells that the reset
    /// reason is the wake from deep sleep.
    pub fn boot(&mut self, deep_sleep_wake: bool) -> Option<Pending> {
        if !self.usable() {
            self.clear(0);
            return None;
        }

        let intact = self.intact();
        let expected = deep_sleep_wake || self.flags & PLANNED != 0;
        if expected {
            self.settle(intact, self.streak);
            return None;
        }

        let streak = self.streak.saturating_add(1);
        if self.streak >= STREAK_RECORDS {
            self.settle(intact, streak);
            return None;
        }

        let pending = Pending {
            suspect: !intact,
            panic: self.flags & PANIC != 0,
        };
        self.set_state(self.flags, streak);

        Some(pending)
    }

    /// Makes the ring ready for new logs.
    fn settle(&mut self, intact: bool, streak: u8) {
        if intact {
            self.set_state(0, streak);
        } else {
            self.clear(streak);
        }
    }

    /// The store has the record, or the record was dropped.
    pub fn saved(&mut self, pending: Pending) {
        self.settle(!pending.suspect, self.streak);
    }

    pub fn push(&mut self, byte: u8) {
        let slot = self.write as usize;
        let next = if slot + 1 == N { 0 } else { self.write + 1 };

        let old = self.data[slot];
        let data_delta =
            Self::slot_weight(slot).wrapping_mul(u32::from(byte).wrapping_sub(u32::from(old)));
        let write_delta = WRITE_WEIGHT.wrapping_mul(next.wrapping_sub(self.write));

        // A reset between these writes must leave a ring that is not intact: data, then `write`,
        // then `check`.
        self.data[slot] = byte;
        self.write = next;
        self.check = self
            .check
            .wrapping_add(data_delta)
            .wrapping_add(write_delta);
    }

    /// The boot works. The next unexpected boot starts a new streak.
    pub fn mark_healthy(&mut self) {
        self.set_state(self.flags, 0);
    }

    /// The next reset is one that the firmware wants.
    pub fn planned_reset(&mut self) {
        self.set_state(self.flags | PLANNED, self.streak);
    }

    pub fn panicked(&mut self) {
        self.set_state(self.flags | PANIC, self.streak);
    }

    /// The ring bytes in two parts. The oldest byte is the first byte of the first part.
    fn oldest_first(&self) -> (&[u8], &[u8]) {
        let (newer, older) = self.data.split_at(self.write as usize);
        (older, newer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Ring = LogRing<64>;

    fn booted() -> Ring {
        let mut ring = Ring::CLEARED;
        assert_eq!(None, ring.boot(true));
        ring
    }

    fn push_noise(ring: &mut Ring, count: usize) {
        let mut state = 0x1234_5678u32;
        for _ in 0..count {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            ring.push((state >> 24) as u8);
        }
    }

    #[test]
    fn kept_checksum_follows_pushes_without_and_with_a_wrap() {
        for count in [0, 1, 63, 64, 65, 1000] {
            let mut ring = booted();
            push_noise(&mut ring, count);

            assert!(ring.intact(), "{count} pushes");
        }
    }

    #[test]
    fn kept_checksum_follows_state_changes() {
        let mut ring = booted();
        push_noise(&mut ring, 100);

        ring.panicked();
        ring.planned_reset();
        ring.mark_healthy();

        assert!(ring.intact());
    }

    #[test]
    fn changed_byte_is_found() {
        let mut ring = booted();
        push_noise(&mut ring, 100);

        ring.data[17] ^= 0x04;

        assert!(!ring.intact());
    }

    #[test]
    fn swap_of_adjacent_bytes_is_found() {
        let mut ring = booted();
        push_noise(&mut ring, 100);
        assert_ne!(ring.data[17], ring.data[18]);

        ring.data.swap(17, 18);

        assert!(!ring.intact());
    }

    #[test]
    fn changed_header_field_is_found() {
        let changes: [fn(&mut Ring); 3] = [
            |ring| ring.write ^= 1,
            |ring| ring.flags ^= PANIC,
            |ring| ring.streak += 1,
        ];

        for change in changes {
            let mut ring = booted();
            push_noise(&mut ring, 100);

            change(&mut ring);

            assert!(!ring.intact());
        }
    }

    #[test]
    fn ring_with_bad_header_is_cleared_without_a_record() {
        let breaks: [fn(&mut Ring); 3] = [
            |ring| ring.magic = 0,
            |ring| ring.write = 64,
            |ring| ring.reserved = [0, 1],
        ];

        for break_header in breaks {
            let mut ring = booted();
            push_noise(&mut ring, 100);
            break_header(&mut ring);

            assert_eq!(None, ring.boot(false));
            assert_eq!([0; 64], ring.data);
            assert!(ring.intact());
        }
    }

    #[test]
    fn wake_from_deep_sleep_keeps_the_logs_without_a_record() {
        let mut ring = booted();
        push_noise(&mut ring, 10);
        let data = ring.data;

        assert_eq!(None, ring.boot(true));
        assert_eq!(data, ring.data);
    }

    #[test]
    fn planned_reset_gives_no_record_once() {
        let mut ring = booted();
        ring.planned_reset();

        assert_eq!(None, ring.boot(false));
        assert!(ring.boot(false).is_some());
    }

    #[test]
    fn expected_boot_clears_a_ring_that_is_not_intact() {
        let mut ring = booted();
        push_noise(&mut ring, 10);
        ring.data[3] ^= 1;

        assert_eq!(None, ring.boot(true));
        assert_eq!([0; 64], ring.data);
        assert!(ring.intact());
    }

    #[test]
    fn unexpected_boot_gives_a_record() {
        let mut ring = booted();
        push_noise(&mut ring, 10);

        let pending = ring.boot(false);

        let expected = Pending {
            suspect: false,
            panic: false,
        };
        assert_eq!(Some(expected), pending);
        assert!(ring.intact());
    }

    #[test]
    fn panic_is_in_the_record_and_gone_after_the_save() {
        let mut ring = booted();
        ring.panicked();

        let pending = ring.boot(false).unwrap();
        ring.saved(pending);

        assert!(pending.panic);
        assert!(!ring.boot(false).unwrap().panic);
    }

    #[test]
    fn ring_that_is_not_intact_gives_a_suspect_record_and_is_cleared_after_the_save() {
        let mut ring = booted();
        push_noise(&mut ring, 10);
        ring.data[3] ^= 1;

        let pending = ring.boot(false).unwrap();
        assert!(pending.suspect);
        assert_ne!([0; 64], ring.data);

        ring.saved(pending);
        assert_eq!([0; 64], ring.data);
        assert!(ring.intact());
    }

    #[test]
    fn streak_gives_four_records_until_a_healthy_point() {
        let mut ring = booted();

        let mut records = 0;
        for _ in 0..6 {
            if let Some(pending) = ring.boot(false) {
                ring.saved(pending);
                records += 1;
            }
        }
        assert_eq!(4, records);

        ring.mark_healthy();
        assert!(ring.boot(false).is_some());
    }

    #[test]
    fn oldest_byte_comes_first() {
        let mut ring = LogRing::<4>::CLEARED;
        ring.boot(true);
        for byte in 1..=6 {
            ring.push(byte);
        }

        assert_eq!((&[3, 4][..], &[5, 6][..]), ring.oldest_first());
    }
}
