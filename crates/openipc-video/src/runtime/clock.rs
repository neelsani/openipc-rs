/// Extends wrapping 32-bit RTP timestamps into a monotonic 64-bit timeline.
#[derive(Debug, Clone, Copy, Default)]
pub struct RtpTimestampUnwrapper {
    last: Option<u32>,
    epoch: i64,
}

impl RtpTimestampUnwrapper {
    /// Extend one timestamp while tolerating normal short packet reordering.
    pub fn unwrap(&mut self, timestamp: u32) -> i64 {
        if let Some(last) = self.last {
            let forward = timestamp.wrapping_sub(last);
            let backward = last.wrapping_sub(timestamp);
            if timestamp < last && forward < 0x8000_0000 {
                self.epoch += 1_i64 << 32;
            } else if timestamp > last && backward < 0x8000_0000 && self.epoch >= 1_i64 << 32 {
                return self.epoch - (1_i64 << 32) + i64::from(timestamp);
            }
        }
        self.last = Some(timestamp);
        self.epoch + i64::from(timestamp)
    }
}

#[cfg(test)]
mod tests {
    use super::RtpTimestampUnwrapper;

    #[test]
    fn extends_timestamp_wrap() {
        let mut clock = RtpTimestampUnwrapper::default();
        assert_eq!(clock.unwrap(u32::MAX - 5), i64::from(u32::MAX - 5));
        assert_eq!(clock.unwrap(4), (1_i64 << 32) + 4);
    }
}
