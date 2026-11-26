//! Debouncing of a digital input.

/// Follows a digital input, and reports a new level only after the input has kept that level for
/// a minimum time. Times are in milliseconds, from any monotonic clock.
pub struct Debouncer {
    stable: bool,
    min_stable_ms: u64,
    /// When the input changed to the level opposite to `stable`, if it has kept that level since.
    changed_at: Option<u64>,
}

impl Debouncer {
    pub const fn new(initial: bool, min_stable_ms: u64) -> Self {
        Self {
            stable: initial,
            min_stable_ms,
            changed_at: None,
        }
    }

    /// Takes a sample of the input, and returns the debounced level.
    pub fn update(&mut self, level: bool, now_ms: u64) -> bool {
        if level == self.stable {
            self.changed_at = None;
        } else {
            let changed_at = *self.changed_at.get_or_insert(now_ms);
            if now_ms - changed_at >= self.min_stable_ms {
                self.stable = level;
                self.changed_at = None;
            }
        }

        self.stable
    }
}

#[cfg(test)]
mod test {
    use super::Debouncer;

    #[test]
    fn keeps_the_initial_level() {
        let mut debouncer = Debouncer::new(true, 100);
        assert!(debouncer.update(true, 0));
        assert!(debouncer.update(true, 1000));
    }

    #[test]
    fn changes_after_the_level_is_stable() {
        let mut debouncer = Debouncer::new(false, 100);
        assert!(!debouncer.update(true, 0));
        assert!(!debouncer.update(true, 99));
        assert!(debouncer.update(true, 100));
    }

    #[test]
    fn ignores_bouncing() {
        let mut debouncer = Debouncer::new(false, 100);
        for t in (0..1000).step_by(20) {
            let level = (t / 20) % 2 == 0;
            assert!(!debouncer.update(level, t));
        }
    }

    #[test]
    fn a_bounce_restarts_the_wait() {
        let mut debouncer = Debouncer::new(false, 100);
        assert!(!debouncer.update(true, 0));
        assert!(!debouncer.update(false, 90));
        assert!(!debouncer.update(true, 100));
        assert!(!debouncer.update(true, 199));
        assert!(debouncer.update(true, 200));
    }

    #[test]
    fn changes_back() {
        let mut debouncer = Debouncer::new(false, 100);
        debouncer.update(true, 0);
        assert!(debouncer.update(true, 100));
        assert!(debouncer.update(false, 150));
        assert!(!debouncer.update(false, 250));
    }
}
