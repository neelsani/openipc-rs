//! One-way synchronization between remote TX and local RX hardware TSF clocks.

/// Least-squares clock fit using beacon TX-egress and local RX timestamps.
///
/// A beaconing transmitter inserts its 64-bit TSF into the frame in hardware,
/// while Realtek RX descriptors carry the receiving adapter's 32-bit TSF low
/// word. Feeding those pairs here fits `local ~= slope * remote + offset` and
/// reconstructs the local clock across its roughly 71-minute low-word wrap.
#[derive(Debug, Clone, Default)]
pub struct TsfSync {
    initialized: bool,
    local_initialized: bool,
    x0: f64,
    y0: f64,
    sx: f64,
    sy: f64,
    sxx: f64,
    sxy: f64,
    samples: u64,
    local_high: i64,
    previous_local_low: u32,
    last_remote: i64,
    last_local: i64,
}

impl TsfSync {
    /// Add one `{remote TX egress, local RX arrival}` timestamp pair in microseconds.
    pub fn add(&mut self, remote_tsf: u64, local_tsfl: u32) {
        let local = self.reconstruct_local(local_tsfl);
        let remote = remote_tsf as i64;
        if !self.initialized {
            self.x0 = remote as f64;
            self.y0 = local as f64;
            self.initialized = true;
        }
        let x = remote as f64 - self.x0;
        let y = local as f64 - self.y0;
        self.samples = self.samples.saturating_add(1);
        self.sx += x;
        self.sy += y;
        self.sxx += x * x;
        self.sxy += x * y;
        self.last_remote = remote;
        self.last_local = local;
    }

    /// Return whether the fit has Devourer's minimum 16 samples.
    pub const fn is_ready(&self) -> bool {
        self.samples >= 16
    }

    /// Number of timestamp pairs in the fit.
    pub const fn sample_count(&self) -> u64 {
        self.samples
    }

    /// Fitted local clock rate relative to the transmitter in parts per million.
    pub fn skew_ppm(&self) -> f64 {
        (self.slope() - 1.0) * 1_000_000.0
    }

    /// Fitted slope of local TSF versus remote TSF.
    pub fn slope(&self) -> f64 {
        let n = self.samples as f64;
        let denominator = n * self.sxx - self.sx * self.sx;
        if denominator == 0.0 {
            1.0
        } else {
            (n * self.sxy - self.sx * self.sy) / denominator
        }
    }

    /// Translate a remote hardware TSF into the equivalent local TSF.
    pub fn local_for_remote(&self, remote_tsf: u64) -> i64 {
        let slope = self.slope();
        let intercept = self.intercept(slope);
        (self.y0 + slope * (remote_tsf as f64 - self.x0) + intercept).round() as i64
    }

    /// Translate a local hardware TSF into the equivalent remote TSF.
    pub fn remote_for_local(&self, local_tsf: u64) -> i64 {
        let slope = self.slope();
        if slope == 0.0 {
            return local_tsf as i64;
        }
        let intercept = self.intercept(slope);
        (self.x0 + ((local_tsf as f64 - self.y0) - intercept) / slope).round() as i64
    }

    /// Latest instantaneous `local - remote` clock offset in microseconds.
    pub const fn offset_us(&self) -> i64 {
        self.last_local - self.last_remote
    }

    fn intercept(&self, slope: f64) -> f64 {
        (self.sy - slope * self.sx) / (self.samples.max(1) as f64)
    }

    fn reconstruct_local(&mut self, low: u32) -> i64 {
        if self.local_initialized && low < self.previous_local_low {
            self.local_high += 1_i64 << 32;
        }
        self.previous_local_low = low;
        self.local_initialized = true;
        self.local_high + i64::from(low)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fits_offset_and_clock_skew() {
        let mut sync = TsfSync::default();
        for sample in 0..32u64 {
            let remote = 1_000_000 + sample * 100_000;
            let local = (remote as f64 * 1.000_025).round() as u64 + 4_000;
            sync.add(remote, local as u32);
        }
        assert!(sync.is_ready());
        assert_eq!(sync.sample_count(), 32);
        assert!((sync.skew_ppm() - 25.0).abs() < 0.1);
        assert!((sync.local_for_remote(5_000_000) - 5_004_125).abs() <= 1);
    }

    #[test]
    fn reconstructs_local_low_word_wrap() {
        let mut sync = TsfSync::default();
        sync.add(10, u32::MAX - 5);
        sync.add(20, 4);
        assert_eq!(sync.offset_us(), (1_i64 << 32) + 4 - 20);
    }
}
