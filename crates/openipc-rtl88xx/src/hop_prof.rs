use std::fmt::Write;

use crate::time::monotonic_micros;

pub(crate) struct HopProfiler {
    generation: &'static str,
    channel: u8,
    started_us: u64,
    last_us: u64,
    stages: Option<Vec<(&'static str, u64)>>,
    force_info: bool,
}

impl HopProfiler {
    pub(crate) fn new(generation: &'static str, channel: u8) -> Self {
        let force_info = env_enabled();
        let enabled =
            force_info || log::log_enabled!(target: "openipc_rtl88xx::hop_prof", log::Level::Trace);
        let now = monotonic_micros();
        Self {
            generation,
            channel,
            started_us: now,
            last_us: now,
            stages: enabled.then(Vec::new),
            force_info,
        }
    }

    pub(crate) fn mark(&mut self, stage: &'static str) {
        let Some(stages) = self.stages.as_mut() else {
            return;
        };
        let now = monotonic_micros();
        stages.push((stage, now.saturating_sub(self.last_us)));
        self.last_us = now;
    }
}

impl Drop for HopProfiler {
    fn drop(&mut self) {
        let Some(stages) = self.stages.as_ref() else {
            return;
        };
        let mut report = format!("gen={} ch={}", self.generation, self.channel);
        for (stage, micros) in stages {
            let _ = write!(report, " {stage}_us={micros}");
        }
        let total = monotonic_micros().saturating_sub(self.started_us);
        if self.force_info {
            log::info!(target: "openipc_rtl88xx::hop_prof", "{report} total_us={total}");
        } else {
            log::trace!(target: "openipc_rtl88xx::hop_prof", "{report} total_us={total}");
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn env_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var_os("DEVOURER_HOP_PROF").is_some()
            || std::env::var_os("OPENIPC_HOP_PROF").is_some()
    })
}

#[cfg(target_arch = "wasm32")]
const fn env_enabled() -> bool {
    false
}
