//! SNTP version 4 (RFC 4330) client packets.

pub const PORT: u16 = 123;
pub const PACKET_LEN: usize = 48;

/// Seconds from 1900-01-01, the NTP epoch, to 1970-01-01.
const UNIX_EPOCH: u64 = 2_208_988_800;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum SntpError {
    Short,
    Mode,
    Stratum,
    /// The reply does not answer the request with this nonce.
    Origin,
    NoTime,
}

/// A client request whose transmit time stamp holds `nonce`. The server copies it into the
/// origin time stamp of its reply.
pub fn request(nonce: u64) -> [u8; PACKET_LEN] {
    let mut packet = [0; PACKET_LEN];
    // Leap indicator 0, version 4, mode 3 (client).
    packet[0] = 0x23;
    packet[40..48].copy_from_slice(&nonce.to_be_bytes());
    packet
}

/// The transmit time of the server, in Unix microseconds.
pub fn parse(reply: &[u8], nonce: u64) -> Result<u64, SntpError> {
    let Some(reply) = reply.get(..PACKET_LEN) else {
        return Err(SntpError::Short);
    };

    if reply[0] & 0x07 != 4 {
        return Err(SntpError::Mode);
    }
    if !(1..=15).contains(&reply[1]) {
        return Err(SntpError::Stratum);
    }
    if reply[24..32] != nonce.to_be_bytes() {
        return Err(SntpError::Origin);
    }

    let seconds = u64::from(u32::from_be_bytes(unwrap!(reply[40..44].try_into().ok())));
    let fraction = u64::from(u32::from_be_bytes(unwrap!(reply[44..48].try_into().ok())));
    if seconds == 0 && fraction == 0 {
        return Err(SntpError::NoTime);
    }

    // Era 0 ends on 2036-02-07. A time stamp with the top bit clear is in era 1.
    let seconds = if seconds & 0x8000_0000 == 0 {
        seconds + (1 << 32)
    } else {
        seconds
    };
    let micros = (fraction * 1_000_000) >> 32;

    Ok((seconds - UNIX_EPOCH) * 1_000_000 + micros)
}

#[cfg(test)]
mod test {
    use super::*;

    const NONCE: u64 = 0x0123_4567_89ab_cdef;

    fn reply(seconds: u32, fraction: u32) -> [u8; PACKET_LEN] {
        let mut packet = [0; PACKET_LEN];
        // Leap indicator 0, version 4, mode 4 (server), stratum 2.
        packet[0] = 0x24;
        packet[1] = 2;
        packet[24..32].copy_from_slice(&NONCE.to_be_bytes());
        packet[40..44].copy_from_slice(&seconds.to_be_bytes());
        packet[44..48].copy_from_slice(&fraction.to_be_bytes());
        packet
    }

    // 2026-01-01T00:00:00Z
    const NEW_YEAR_2026_NTP: u32 = 3_976_214_400;
    const NEW_YEAR_2026_US: u64 = 1_767_225_600_000_000;

    #[test]
    fn request_carries_the_version_mode_and_nonce() {
        let packet = request(NONCE);

        assert_eq!(0x23, packet[0]);
        assert_eq!(NONCE.to_be_bytes(), packet[40..48]);
        assert!(packet[1..40].iter().all(|byte| *byte == 0));
    }

    #[test]
    fn reply_gives_unix_microseconds() {
        let half_second = 1 << 31;

        assert_eq!(
            Ok(NEW_YEAR_2026_US + 500_000),
            parse(&reply(NEW_YEAR_2026_NTP, half_second), NONCE)
        );
    }

    #[test]
    fn time_stamp_with_top_bit_clear_is_in_era_1() {
        // 2036-02-07T06:28:17Z, one second into era 1.
        assert_eq!(Ok(2_085_978_497_000_000), parse(&reply(1, 0), NONCE));
    }

    #[test]
    fn bad_replies_are_refused() {
        let good = reply(NEW_YEAR_2026_NTP, 0);

        assert_eq!(Err(SntpError::Short), parse(&good[..47], NONCE));

        let mut client = good;
        client[0] = 0x23;
        assert_eq!(Err(SntpError::Mode), parse(&client, NONCE));

        let mut kiss_of_death = good;
        kiss_of_death[1] = 0;
        assert_eq!(Err(SntpError::Stratum), parse(&kiss_of_death, NONCE));

        let mut unsynchronized = good;
        unsynchronized[1] = 16;
        assert_eq!(Err(SntpError::Stratum), parse(&unsynchronized, NONCE));

        assert_eq!(Err(SntpError::Origin), parse(&good, NONCE + 1));

        assert_eq!(Err(SntpError::NoTime), parse(&reply(0, 0), NONCE));
    }
}
