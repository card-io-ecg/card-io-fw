use core::fmt::Write;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum QueueError {
    WrongLength,
    HeadAfterTail,
    Overflow,
}

/// Saved measurements are `head..tail`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Queue {
    pub head: u32,
    pub tail: u32,
}

impl Queue {
    pub const KEY: &[u8] = b"queue";

    pub fn decode(bytes: &[u8]) -> Result<Self, QueueError> {
        let &[h0, h1, h2, h3, t0, t1, t2, t3] = bytes else {
            return Err(QueueError::WrongLength);
        };

        let head = u32::from_le_bytes([h0, h1, h2, h3]);
        let tail = u32::from_le_bytes([t0, t1, t2, t3]);
        if head > tail {
            return Err(QueueError::HeadAfterTail);
        }

        Ok(Self { head, tail })
    }

    pub fn encode(&self) -> [u8; 8] {
        let [h0, h1, h2, h3] = self.head.to_le_bytes();
        let [t0, t1, t2, t3] = self.tail.to_le_bytes();
        [h0, h1, h2, h3, t0, t1, t2, t3]
    }

    pub fn is_empty(&self) -> bool {
        self.head == self.tail
    }

    pub fn len(&self) -> u32 {
        self.tail - self.head
    }

    pub fn after_push(self) -> Result<Self, QueueError> {
        let tail = self.tail.checked_add(1).ok_or(QueueError::Overflow)?;
        Ok(Self { tail, ..self })
    }

    /// The IDs restart from 0 when the last measurement is removed.
    pub fn after_pop(self) -> Self {
        if self.len() <= 1 {
            return Self::default();
        }

        Self {
            head: self.head + 1,
            ..self
        }
    }
}

pub fn measurement_key(id: u32) -> heapless::String<16> {
    let mut key = heapless::String::new();
    unwrap!(write!(key, "meas/{id:08}").ok());
    key
}

pub fn measurement_version_key(id: u32) -> heapless::String<16> {
    let mut key = heapless::String::new();
    unwrap!(write!(key, "ver/{id:08}").ok());
    key
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn queue_round_trips() {
        let queue = Queue {
            head: 3,
            tail: 70_000,
        };

        assert_eq!(Ok(queue), Queue::decode(&queue.encode()));
    }

    #[test]
    fn queue_of_wrong_length_is_rejected() {
        assert_eq!(Err(QueueError::WrongLength), Queue::decode(&[0; 7]));
        assert_eq!(Err(QueueError::WrongLength), Queue::decode(&[0; 9]));
        assert_eq!(Err(QueueError::WrongLength), Queue::decode(&[]));
    }

    #[test]
    fn queue_with_head_after_tail_is_rejected() {
        let bytes = Queue { head: 2, tail: 1 }.encode();

        assert_eq!(Err(QueueError::HeadAfterTail), Queue::decode(&bytes));
    }

    #[test]
    fn push_extends_the_queue_at_the_tail() {
        let queue = Queue { head: 1, tail: 4 }.after_push().unwrap();

        assert_eq!(Queue { head: 1, tail: 5 }, queue);
    }

    #[test]
    fn push_fails_when_the_tail_cannot_grow() {
        let queue = Queue {
            head: 0,
            tail: u32::MAX,
        };

        assert_eq!(Err(QueueError::Overflow), queue.after_push());
    }

    #[test]
    fn pop_removes_the_oldest_measurement() {
        let queue = Queue { head: 1, tail: 4 }.after_pop();

        assert_eq!(Queue { head: 2, tail: 4 }, queue);
    }

    #[test]
    fn pop_of_the_last_measurement_restarts_the_ids() {
        let queue = Queue { head: 6, tail: 7 }.after_pop();

        assert_eq!(Queue { head: 0, tail: 0 }, queue);
    }

    #[test]
    fn len_and_is_empty_follow_head_and_tail() {
        let empty = Queue { head: 5, tail: 5 };
        let two = Queue { head: 5, tail: 7 };

        assert!(empty.is_empty());
        assert_eq!(0, empty.len());
        assert!(!two.is_empty());
        assert_eq!(2, two.len());
    }

    #[test]
    fn measurement_key_is_zero_padded() {
        assert_eq!("meas/00000007", measurement_key(7).as_str());
    }

    #[test]
    fn longest_measurement_key_fits() {
        assert_eq!("meas/4294967295", measurement_key(u32::MAX).as_str());
    }

    #[test]
    fn measurement_version_key_is_zero_padded() {
        assert_eq!("ver/00000007", measurement_version_key(7).as_str());
    }

    #[test]
    fn longest_measurement_version_key_fits() {
        assert_eq!("ver/4294967295", measurement_version_key(u32::MAX).as_str());
    }

    #[test]
    fn keys_sort_as_the_store_transactions_write_them() {
        let id = 7;
        let keys = [
            measurement_key(id),
            heapless::String::<16>::try_from("queue").unwrap(),
            measurement_version_key(id),
        ];

        assert!(keys.is_sorted());
        assert_eq!(Queue::KEY, keys[1].as_bytes());
    }
}
