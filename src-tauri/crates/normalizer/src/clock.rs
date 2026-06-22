/// Monotonic clock corrector.
///
/// The capture backend may produce timestamps that jump backwards (clock
/// resets, CPU migration, TSC skew).  This corrector detects and repairs
/// those discontinuities by clamping to the last-seen timestamp and counting
/// how many corrections were applied.
///
/// Correction policy: when `t < last`, clamp to `last` (not `last + 1`).
/// We don't know the real timestamp; preserving ordering without creating
/// phantom gaps is preferable to guessing.
#[derive(Debug, Default)]
pub struct ClockCorrector {
    last_ns: Option<u64>,
    pub corrections: u64,
}

impl ClockCorrector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Accept a raw timestamp and return a corrected, monotonically non-decreasing one.
    pub fn correct(&mut self, ts: u64) -> u64 {
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

    pub fn reset(&mut self) {
        self.last_ns = None;
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
        assert_eq!(c.correct(300), 300);
        assert_eq!(c.corrections, 0);
    }

    #[test]
    fn backwards_clamped() {
        let mut c = ClockCorrector::new();
        c.correct(200);
        let fixed = c.correct(100);
        assert_eq!(fixed, 200);
        assert_eq!(c.corrections, 1);
    }

    #[test]
    fn equal_timestamp_not_counted() {
        let mut c = ClockCorrector::new();
        c.correct(100);
        let result = c.correct(100);
        assert_eq!(result, 100);
        assert_eq!(c.corrections, 0);
    }
}
