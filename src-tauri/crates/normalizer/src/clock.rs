/// Monotonic clock corrector for `i64` nanosecond timestamps.
///
/// Capture backends may produce non-monotone timestamps due to TSC skew,
/// CPU migration, or clock resets.  This corrector clamps backwards jumps
/// to the last-seen value and counts how many corrections were applied.
#[derive(Debug, Default)]
pub struct ClockCorrector {
    last_ns: Option<i64>,
    pub corrections: u64,
}

impl ClockCorrector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Accept a raw timestamp; return a monotonically non-decreasing value.
    pub fn correct(&mut self, ts: i64) -> i64 {
        match self.last_ns {
            None => {
                self.last_ns = Some(ts);
                ts
            }
            Some(last) => {
                if ts < last {
                    self.corrections += 1;
                    last
                } else {
                    self.last_ns = Some(ts);
                    ts
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monotone_passthrough() {
        let mut c = ClockCorrector::new();
        assert_eq!(c.correct(100), 100);
        assert_eq!(c.correct(200), 200);
        assert_eq!(c.corrections, 0);
    }

    #[test]
    fn backwards_clamped() {
        let mut c = ClockCorrector::new();
        c.correct(200);
        assert_eq!(c.correct(100), 200);
        assert_eq!(c.corrections, 1);
    }

    #[test]
    fn equal_not_counted() {
        let mut c = ClockCorrector::new();
        c.correct(100);
        assert_eq!(c.correct(100), 100);
        assert_eq!(c.corrections, 0);
    }

    #[test]
    fn handles_negative_timestamps() {
        let mut c = ClockCorrector::new();
        assert_eq!(c.correct(-500), -500);
        assert_eq!(c.correct(-300), -300);
        assert_eq!(c.correct(-400), -300);
        assert_eq!(c.corrections, 1);
    }
}
