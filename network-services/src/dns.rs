//! The DNS server of the setup access point. Every name is the access point, so a phone that checks
//! for internet reaches the setup page and opens it as a captive portal.

use core::net::Ipv4Addr;

const HEADER: usize = 12;
const TYPE_A: u16 = 1;
const CLASS_IN: u16 = 1;
/// How long a client may keep an answer, in seconds.
const TTL: u32 = 60;

const RESPONSE: u16 = 0x8000;
const OPCODE: u16 = 0x7800;
const AUTHORITATIVE: u16 = 0x0400;
const RECURSION_DESIRED: u16 = 0x0100;
const RECURSION_AVAILABLE: u16 = 0x0080;

/// Writes the answer to `query` into `out` and returns its length. A question for an A record gets
/// `address`; any other question gets an answer without records. `None` for a message that is not
/// a standard query with one question, or an `out` that is too short.
pub fn answer(query: &[u8], address: Ipv4Addr, out: &mut [u8]) -> Option<usize> {
    let header = query.get(..HEADER)?;
    let flags = u16::from_be_bytes([header[2], header[3]]);
    let questions = u16::from_be_bytes([header[4], header[5]]);
    if flags & (RESPONSE | OPCODE) != 0 || questions != 1 {
        return None;
    }

    // The question: the labels of the name up to an empty one, then the type and the class.
    let mut end = HEADER;
    loop {
        let len = usize::from(*query.get(end)?);
        end += 1;
        if len == 0 {
            break;
        }
        // A question has no compressed names.
        if len & 0xc0 != 0 {
            return None;
        }
        end += len;
    }
    let fields = query.get(end..end + 4)?;
    let kind = u16::from_be_bytes([fields[0], fields[1]]);
    let class = u16::from_be_bytes([fields[2], fields[3]]);
    let question = &query[HEADER..end + 4];

    let records: u16 = u16::from(kind == TYPE_A && class == CLASS_IN);
    let len = HEADER + question.len() + usize::from(records) * 16;
    let out = out.get_mut(..len)?;

    let flags = RESPONSE | AUTHORITATIVE | (flags & RECURSION_DESIRED) | RECURSION_AVAILABLE;
    out[0..2].copy_from_slice(&header[0..2]);
    out[2..4].copy_from_slice(&flags.to_be_bytes());
    out[4..6].copy_from_slice(&1u16.to_be_bytes());
    out[6..8].copy_from_slice(&records.to_be_bytes());
    out[8..HEADER].fill(0);
    out[HEADER..HEADER + question.len()].copy_from_slice(question);

    if records == 1 {
        let record = &mut out[HEADER + question.len()..];
        // A pointer to the name of the question, at the end of the header.
        record[0..2].copy_from_slice(&(0xc000 | HEADER as u16).to_be_bytes());
        record[2..4].copy_from_slice(&TYPE_A.to_be_bytes());
        record[4..6].copy_from_slice(&CLASS_IN.to_be_bytes());
        record[6..10].copy_from_slice(&TTL.to_be_bytes());
        record[10..12].copy_from_slice(&4u16.to_be_bytes());
        record[12..16].copy_from_slice(&address.octets());
    }

    Some(len)
}

#[cfg(test)]
mod test {
    use super::*;
    use std::vec::Vec;

    const PORTAL: Ipv4Addr = Ipv4Addr::new(192, 168, 2, 1);

    fn query(name: &str, kind: u16) -> Vec<u8> {
        // ID 0xbeef, recursion desired, one question.
        let mut message = std::vec![0xbe, 0xef, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0];
        for label in name.split('.') {
            message.push(label.len() as u8);
            message.extend_from_slice(label.as_bytes());
        }
        message.push(0);
        message.extend_from_slice(&kind.to_be_bytes());
        message.extend_from_slice(&CLASS_IN.to_be_bytes());
        message
    }

    fn reply(query: &[u8]) -> Option<Vec<u8>> {
        let mut out = [0; 512];
        let len = answer(query, PORTAL, &mut out)?;
        Some(out[..len].to_vec())
    }

    #[test]
    fn an_a_question_gets_the_portal_address() {
        let query = query("connectivitycheck.gstatic.com", TYPE_A);
        let reply = reply(&query).unwrap();

        assert_eq!(reply[0..2], [0xbe, 0xef]);
        // Response, authoritative, recursion desired and available.
        assert_eq!(reply[2..4], [0x85, 0x80]);
        assert_eq!(reply[4..12], [0, 1, 0, 1, 0, 0, 0, 0]);
        assert_eq!(reply[12..query.len()], query[12..]);
        assert_eq!(
            reply[query.len()..],
            [0xc0, 0x0c, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 192, 168, 2, 1]
        );
    }

    #[test]
    fn another_question_gets_no_records() {
        let query = query("connectivitycheck.gstatic.com", 28);
        let reply = reply(&query).unwrap();

        assert_eq!(reply[4..12], [0, 1, 0, 0, 0, 0, 0, 0]);
        assert_eq!(reply.len(), query.len());
    }

    #[test]
    fn messages_that_are_not_one_standard_query_get_no_answer() {
        let good = query("example.com", TYPE_A);

        let mut response = good.clone();
        response[2] |= 0x80;
        assert_eq!(reply(&response), None);

        let mut inverse = good.clone();
        inverse[2] |= 0x08;
        assert_eq!(reply(&inverse), None);

        let mut two_questions = good.clone();
        two_questions[5] = 2;
        assert_eq!(reply(&two_questions), None);

        assert_eq!(reply(&good[..good.len() - 1]), None);
        assert_eq!(reply(&good[..11]), None);
    }

    #[test]
    fn a_short_buffer_gets_no_answer() {
        let query = query("example.com", TYPE_A);
        let mut out = [0; 20];
        assert_eq!(answer(&query, PORTAL, &mut out), None);
    }
}
