//! Closed-loop carrier-frequency-offset controller.

/// Result of one periodic CFO control tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CfoStep {
    /// Mean path-A CFO using Realtek's nominal `raw * 2.5 kHz` conversion.
    pub average_khz: f64,
    /// New crystal-cap code, or `None` when no register change is needed.
    pub crystal_cap: Option<u8>,
}

/// Devourer-compatible bang-bang CFO tracker with hysteresis.
#[derive(Debug, Clone, Default)]
pub struct CfoTracker {
    sum: i64,
    count: u64,
    adjusting: bool,
    inverted: bool,
    stepped: bool,
    previous_magnitude: f64,
}

impl CfoTracker {
    /// Add one path-A OFDM CFO-tail sample from RX PHY status.
    pub fn add(&mut self, cfo_tail: i8) {
        self.sum += i64::from(cfo_tail);
        self.count = self.count.saturating_add(1);
    }

    /// Drain accumulated samples and select at most one crystal-cap step.
    pub fn step(&mut self, current_cap: u8, cap_max: u8) -> Option<CfoStep> {
        if self.count == 0 {
            return None;
        }
        let average_khz = self.sum as f64 / self.count as f64 * 2.5;
        self.sum = 0;
        self.count = 0;
        let magnitude = average_khz.abs();

        if self.stepped && magnitude > self.previous_magnitude + 1.0 {
            self.inverted = !self.inverted;
        }
        self.stepped = false;

        if !self.adjusting {
            if magnitude > 11.0 {
                self.adjusting = true;
            } else {
                self.previous_magnitude = magnitude;
                return Some(CfoStep {
                    average_khz,
                    crystal_cap: None,
                });
            }
        } else if magnitude <= 10.0 {
            self.adjusting = false;
            self.previous_magnitude = magnitude;
            return Some(CfoStep {
                average_khz,
                crystal_cap: None,
            });
        }

        let positive = (average_khz > 0.0) ^ self.inverted;
        let next = if positive {
            current_cap.saturating_add(1).min(cap_max)
        } else {
            current_cap.saturating_sub(1)
        };
        self.previous_magnitude = magnitude;
        let crystal_cap = (next != current_cap).then_some(next);
        self.stepped = crystal_cap.is_some();
        Some(CfoStep {
            average_khz,
            crystal_cap,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_one_step_beyond_enable_threshold() {
        let mut tracker = CfoTracker::default();
        tracker.add(8);
        tracker.add(8);
        assert_eq!(tracker.step(0x20, 0x3f).unwrap().crystal_cap, Some(0x21));
    }

    #[test]
    fn deadband_and_rails_do_not_request_writes() {
        let mut tracker = CfoTracker::default();
        tracker.add(4);
        assert_eq!(tracker.step(0x20, 0x3f).unwrap().crystal_cap, None);
        tracker.add(8);
        assert_eq!(tracker.step(0x3f, 0x3f).unwrap().crystal_cap, None);
    }

    #[test]
    fn flips_polarity_after_a_worse_step() {
        let mut tracker = CfoTracker::default();
        tracker.add(8);
        assert_eq!(tracker.step(0x20, 0x3f).unwrap().crystal_cap, Some(0x21));
        tracker.add(12);
        assert_eq!(tracker.step(0x21, 0x3f).unwrap().crystal_cap, Some(0x20));
    }
}
