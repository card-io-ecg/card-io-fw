//! The record of one unexpected boot, and where the store keeps it.

use core::ops::Range;

use crate::{LogRing, Pending};

/// The record bytes before the two version strings.
pub const RECORD_HEADER: usize = 16;
pub const MAX_VERSION_LEN: usize = 16;

/// Holds the sequence number of the next record, 4 bytes.
pub const NEXT_KEY: &[u8] = b"log_next";

const MAGIC: [u8; 4] = *b"MLOG";
const FORMAT_VERSION: u8 = 1;
const ENCODING_DEFMT_RZCOBS: u8 = 0;

const SUSPECT: u8 = 1 << 0;
const PANIC: u8 = 1 << 1;

pub struct RecordInfo<'a> {
    pub pending: Pending,
    /// The `SocResetReason` value, or 0 for a reason that esp-hal does not know.
    pub reset_reason: u8,
    /// The count of records that this device saved before.
    pub sequence: u32,
    pub firmware_version: &'a str,
    pub hardware_version: &'a str,
}

/// The key of the record with this sequence number, in a store that keeps the newest `slots`
/// records. `slots` is in `1..=100`. The key sorts before [`NEXT_KEY`].
pub fn record_key(sequence: u32, slots: u8) -> [u8; 6] {
    let slot = (sequence % u32::from(slots)) as u8;
    [b'l', b'o', b'g', b'/', b'0' + slot / 10, b'0' + slot % 10]
}

/// The sequence numbers whose slots can hold a record that waits for upload, oldest first. `next`
/// is the value of [`NEXT_KEY`]. A slot without a record holds no waiting record.
pub fn waiting(next: u32, slots: u8) -> Range<u32> {
    next.saturating_sub(u32::from(slots))..next
}

/// The sequence number of the next record, from the value of [`NEXT_KEY`].
pub fn decode_next(value: Option<&[u8]>) -> u32 {
    value
        .and_then(|value| value.try_into().ok())
        .map(u32::from_le_bytes)
        .unwrap_or(0)
}

/// Writes the record into `out` and returns its length. Returns `None` when a version string is
/// longer than [`MAX_VERSION_LEN`] or `out` is too short.
pub fn encode_record<const N: usize>(
    ring: &LogRing<N>,
    info: &RecordInfo<'_>,
    out: &mut [u8],
) -> Option<usize> {
    let firmware = info.firmware_version.as_bytes();
    let hardware = info.hardware_version.as_bytes();
    if firmware.len() > MAX_VERSION_LEN || hardware.len() > MAX_VERSION_LEN {
        return None;
    }

    let mut flags = 0;
    if info.pending.suspect {
        flags |= SUSPECT;
    }
    if info.pending.panic {
        flags |= PANIC;
    }

    let mut header = [0; RECORD_HEADER];
    header[0..4].copy_from_slice(&MAGIC);
    header[4] = FORMAT_VERSION;
    header[5] = flags;
    header[6] = info.reset_reason;
    header[7] = ENCODING_DEFMT_RZCOBS;
    header[8..12].copy_from_slice(&info.sequence.to_le_bytes());
    header[12..14].copy_from_slice(&u16::try_from(N).ok()?.to_le_bytes());
    header[14] = firmware.len() as u8;
    header[15] = hardware.len() as u8;

    let (older, newer) = ring.oldest_first();
    let parts = [&header[..], firmware, hardware, older, newer];

    let mut len = 0;
    for part in parts {
        out.get_mut(len..len + part.len())?.copy_from_slice(part);
        len += part.len();
    }

    Some(len)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(pending: Pending) -> RecordInfo<'static> {
        RecordInfo {
            pending,
            reset_reason: 15,
            sequence: 0x0102_0304,
            firmware_version: "3f9a1c2",
            hardware_version: "v8c6",
        }
    }

    const PLAIN: Pending = Pending {
        suspect: false,
        panic: false,
    };

    #[test]
    fn record_matches_the_format() {
        let mut ring = LogRing::<4>::CLEARED;
        ring.boot(true);
        for byte in 1..=6 {
            ring.push(byte);
        }
        let pending = Pending {
            suspect: true,
            panic: true,
        };

        let mut out = [0xAA; 64];
        let len = encode_record(&ring, &info(pending), &mut out).unwrap();

        let expected = [
            b'M', b'L', b'O', b'G', 1, 0b11, 15, 0, 4, 3, 2, 1, 4, 0, 7, 4, b'3', b'f', b'9', b'a',
            b'1', b'c', b'2', b'v', b'8', b'c', b'6', 3, 4, 5, 6,
        ];
        assert_eq!(expected, out[..len]);
    }

    #[test]
    fn record_needs_room_and_short_versions() {
        let ring = LogRing::<4>::CLEARED;

        assert_eq!(None, encode_record(&ring, &info(PLAIN), &mut [0; 30]));

        let long = RecordInfo {
            firmware_version: "0123456789abcdef0",
            ..info(PLAIN)
        };
        assert_eq!(None, encode_record(&ring, &long, &mut [0; 64]));
    }

    #[test]
    fn records_take_the_slots_in_turn() {
        assert_eq!(*b"log/00", record_key(0, 32));
        assert_eq!(*b"log/31", record_key(31, 32));
        assert_eq!(*b"log/00", record_key(32, 32));
        assert_eq!(*b"log/99", record_key(199, 100));
        assert!(record_key(99, 100).as_slice() < NEXT_KEY);
    }

    #[test]
    fn the_newest_slots_wait_for_upload() {
        assert_eq!(0..0, waiting(0, 32));
        assert_eq!(0..5, waiting(5, 32));
        assert_eq!(8..40, waiting(40, 32));
    }

    #[test]
    fn missing_or_malformed_next_counts_as_zero() {
        assert_eq!(0, decode_next(None));
        assert_eq!(0, decode_next(Some(&[1, 2, 3])));
        assert_eq!(0x0403_0201, decode_next(Some(&[1, 2, 3, 4])));
    }
}
