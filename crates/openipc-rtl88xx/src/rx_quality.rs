//! Rolling receive-quality aggregation and passive noise-floor estimation.

use std::sync::Mutex;

use openipc_core::realtek::RxPacketAttrib;

use crate::{classify_link_health, LinkHealth, LinkHealthInput, LinkHealthThresholds, RxEnergy};

/// Drained classification of receive chains that currently carry useful signal.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ActiveRxPaths {
    /// Whether at least one frame supplied per-chain PHY status.
    pub valid: bool,
    /// Frames with at least one sampled chain.
    pub frames: u32,
    /// Number of physical chains considered.
    pub chain_count: u8,
    /// Number of chains within the configured margin of the strongest chain.
    pub active_count: u8,
    /// Bit `n` is set when chain `n` is active.
    pub active_mask: u8,
    /// Mean RSSI in dBm for each chain.
    pub rssi_mean_dbm: [i16; 4],
    /// Whether each chain supplied a non-zero PHY-status sample.
    pub chain_sampled: [bool; 4],
}

/// Classify sampled chains relative to the strongest chain.
pub fn classify_active_rx_paths(
    rssi_mean_dbm: [i16; 4],
    sampled: [bool; 4],
    chain_count: u8,
    margin_db: i16,
) -> (u8, u8) {
    let count = chain_count.min(4) as usize;
    let strongest = (0..count)
        .filter(|index| sampled[*index])
        .map(|index| rssi_mean_dbm[index])
        .max();
    let Some(strongest) = strongest else {
        return (0, 0);
    };
    let mut mask = 0u8;
    let mut active = 0u8;
    for index in 0..count {
        if sampled[index] && strongest - rssi_mean_dbm[index] <= margin_db {
            mask |= 1 << index;
            active += 1;
        }
    }
    (active, mask)
}

#[derive(Debug, Default)]
struct RxPathState {
    frames: u32,
    chain_count: u8,
    sums: [u64; 4],
    counts: [u32; 4],
}

/// Thread-safe rolling per-chain RSSI accumulator with drain semantics.
#[derive(Debug, Default)]
pub struct RxPathActivityAccumulator {
    state: Mutex<RxPathState>,
}

impl RxPathActivityAccumulator {
    /// Observe one frame's raw Realtek per-chain RSSI tuple.
    pub fn observe(&self, rssi: [u8; 4], chain_count: u8) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        state.chain_count = chain_count.min(4);
        let mut any = false;
        for (index, value) in rssi
            .into_iter()
            .take(usize::from(state.chain_count))
            .enumerate()
        {
            if value == 0 {
                continue;
            }
            state.sums[index] += u64::from(value);
            state.counts[index] = state.counts[index].saturating_add(1);
            any = true;
        }
        if any {
            state.frames = state.frames.saturating_add(1);
        }
    }

    /// Drain and classify the current receive window.
    pub fn snapshot(&self, margin_db: i16) -> ActiveRxPaths {
        let Ok(mut state) = self.state.lock() else {
            return ActiveRxPaths::default();
        };
        let mut rssi_mean_dbm = [0i16; 4];
        let mut sampled = [false; 4];
        for index in 0..usize::from(state.chain_count) {
            if state.counts[index] == 0 {
                continue;
            }
            rssi_mean_dbm[index] =
                (state.sums[index] / u64::from(state.counts[index])) as i16 - 110;
            sampled[index] = true;
        }
        let (active_count, active_mask) =
            classify_active_rx_paths(rssi_mean_dbm, sampled, state.chain_count, margin_db);
        let snapshot = ActiveRxPaths {
            valid: state.frames != 0,
            frames: state.frames,
            chain_count: state.chain_count,
            active_count,
            active_mask,
            rssi_mean_dbm,
            chain_sampled: sampled,
        };
        *state = RxPathState::default();
        snapshot
    }
}

/// Drained aggregate of one receive window in raw Realtek units.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RxQualitySnapshot {
    /// Number of clean frames included.
    pub frames: u32,
    /// Mean path-A PWDB.
    pub rssi_mean_raw: i16,
    /// Peak path-A PWDB.
    pub rssi_max_raw: i16,
    /// Mean path-A SNR in half-dB.
    pub snr_mean_raw: i16,
    /// Minimum path-A SNR in half-dB.
    pub snr_min_raw: i16,
    /// Mean path-A EVM in half-dB.
    pub evm_mean_raw: i16,
    /// Whether EVM samples were present.
    pub evm_valid: bool,
    /// Mean passive `RSSI - SNR` noise-floor estimate in dBm.
    pub noise_floor_dbm: f32,
    /// Whether SNR-bearing frames produced a noise-floor estimate.
    pub noise_floor_valid: bool,
}

/// Controller-facing fused RX-quality report.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RxQuality {
    /// Whether at least one clean frame was observed.
    pub valid: bool,
    /// Number of frames in this drained window.
    pub frames: u32,
    /// Mean RSSI in dBm.
    pub rssi_mean_dbm: i16,
    /// Peak RSSI in dBm.
    pub rssi_max_dbm: i16,
    /// Mean SNR in dB.
    pub snr_mean_db: f32,
    /// Minimum SNR in dB.
    pub snr_min_db: f32,
    /// Mean EVM in dB.
    pub evm_mean_db: f32,
    /// Whether EVM is available.
    pub evm_valid: bool,
    /// Passive noise-floor estimate in dBm.
    pub noise_floor_dbm: f32,
    /// Whether the passive noise-floor estimate is available.
    pub noise_floor_valid: bool,
    /// Optional frame-free energy snapshot.
    pub energy: Option<RxEnergy>,
    /// Fused link diagnosis.
    pub health: LinkHealth,
}

#[derive(Debug)]
struct AccumulatorState {
    frames: u32,
    rssi_sum: i64,
    rssi_max: i16,
    snr_sum: i64,
    snr_min: i16,
    evm_sum: i64,
    evm_count: u32,
    noise_floor_sum: f64,
    noise_floor_count: u32,
}

impl Default for AccumulatorState {
    fn default() -> Self {
        Self {
            frames: 0,
            rssi_sum: 0,
            rssi_max: i16::MIN,
            snr_sum: 0,
            snr_min: i16::MAX,
            evm_sum: 0,
            evm_count: 0,
            noise_floor_sum: 0.0,
            noise_floor_count: 0,
        }
    }
}

/// Thread-safe rolling accumulator fed once per parsed RX descriptor.
#[derive(Debug, Default)]
pub struct RxQualityAccumulator {
    state: Mutex<AccumulatorState>,
}

impl RxQualityAccumulator {
    /// Observe one clean normal RX packet's descriptor metadata.
    pub fn observe(&self, attrib: &RxPacketAttrib) {
        if attrib.crc_err || attrib.icv_err {
            return;
        }
        self.add_raw(attrib.rssi[0], attrib.snr[0], attrib.evm[0]);
    }

    /// Observe path-A values in the same raw units used by Devourer.
    pub fn add_raw(&self, rssi_raw: u8, snr_raw: i8, evm_raw: i8) {
        if rssi_raw == 0 {
            return;
        }
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let rssi = i16::from(rssi_raw);
        state.frames = state.frames.saturating_add(1);
        state.rssi_sum += i64::from(rssi);
        state.rssi_max = state.rssi_max.max(rssi);
        let snr = i16::from(snr_raw);
        state.snr_sum += i64::from(snr);
        state.snr_min = state.snr_min.min(snr);
        if snr_raw != 0 {
            state.noise_floor_sum += f64::from(rssi - 110) - f64::from(snr) / 2.0;
            state.noise_floor_count = state.noise_floor_count.saturating_add(1);
        }
        if evm_raw != 0 {
            state.evm_sum += i64::from(evm_raw);
            state.evm_count = state.evm_count.saturating_add(1);
        }
    }

    /// Drain and reset the current receive window.
    pub fn snapshot(&self) -> RxQualitySnapshot {
        let Ok(mut state) = self.state.lock() else {
            return RxQualitySnapshot::default();
        };
        let snapshot = RxQualitySnapshot {
            frames: state.frames,
            rssi_mean_raw: average(state.rssi_sum, state.frames),
            rssi_max_raw: if state.frames == 0 { 0 } else { state.rssi_max },
            snr_mean_raw: average(state.snr_sum, state.frames),
            snr_min_raw: if state.frames == 0 { 0 } else { state.snr_min },
            evm_mean_raw: average(state.evm_sum, state.evm_count),
            evm_valid: state.evm_count != 0,
            noise_floor_dbm: if state.noise_floor_count == 0 {
                0.0
            } else {
                (state.noise_floor_sum / f64::from(state.noise_floor_count)) as f32
            },
            noise_floor_valid: state.noise_floor_count != 0,
        };
        *state = AccumulatorState::default();
        snapshot
    }

    /// Drain the window and fuse it with an optional frame-free energy sample.
    pub fn quality(&self, energy: Option<RxEnergy>, thresholds: LinkHealthThresholds) -> RxQuality {
        let snapshot = self.snapshot();
        let health = classify_link_health(
            LinkHealthInput {
                frames: snapshot.frames,
                rssi_raw: snapshot.rssi_max_raw,
                snr_raw: snapshot.snr_mean_raw,
                evm_raw: snapshot.evm_mean_raw,
                evm_valid: snapshot.evm_valid,
                energy_valid: energy.is_some(),
                fa_ofdm: energy.map_or(0, |sample| sample.fa_ofdm),
                cca_ofdm: energy.map_or(0, |sample| sample.cca_ofdm),
                igi_valid: energy.is_some(),
                igi: energy.map_or(0, |sample| sample.igi),
                igi_min: 0x1c,
                igi_max: 0x7f,
            },
            thresholds,
        );
        RxQuality {
            valid: snapshot.frames != 0,
            frames: snapshot.frames,
            rssi_mean_dbm: snapshot.rssi_mean_raw - 110,
            rssi_max_dbm: snapshot.rssi_max_raw - 110,
            snr_mean_db: f32::from(snapshot.snr_mean_raw) / 2.0,
            snr_min_db: f32::from(snapshot.snr_min_raw) / 2.0,
            evm_mean_db: f32::from(snapshot.evm_mean_raw) / 2.0,
            evm_valid: snapshot.evm_valid,
            noise_floor_dbm: snapshot.noise_floor_dbm,
            noise_floor_valid: snapshot.noise_floor_valid,
            energy,
            health,
        }
    }
}

fn average(sum: i64, count: u32) -> i16 {
    if count == 0 {
        0
    } else {
        (sum / i64::from(count)) as i16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregates_and_resets_like_devourer() {
        let accumulator = RxQualityAccumulator::default();
        accumulator.add_raw(70, 30, -50);
        accumulator.add_raw(80, 20, -40);
        let snapshot = accumulator.snapshot();
        assert_eq!(snapshot.frames, 2);
        assert_eq!(snapshot.rssi_mean_raw, 75);
        assert_eq!(snapshot.rssi_max_raw, 80);
        assert_eq!(snapshot.snr_mean_raw, 25);
        assert_eq!(snapshot.evm_mean_raw, -45);
        assert!((snapshot.noise_floor_dbm - -47.5).abs() < 0.01);
        assert_eq!(accumulator.snapshot().frames, 0);
    }

    #[test]
    fn classifies_balanced_and_disconnected_chains() {
        let accumulator = RxPathActivityAccumulator::default();
        accumulator.observe([80, 76, 41, 0], 4);
        accumulator.observe([78, 74, 39, 0], 4);
        let snapshot = accumulator.snapshot(20);
        assert_eq!(snapshot.frames, 2);
        assert_eq!(snapshot.rssi_mean_dbm, [-31, -35, -70, 0]);
        assert_eq!(snapshot.active_count, 2);
        assert_eq!(snapshot.active_mask, 0b0011);
        assert_eq!(accumulator.snapshot(20).frames, 0);
    }
}
