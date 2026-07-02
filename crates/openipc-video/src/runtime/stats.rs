use std::sync::{Arc, Mutex};

use crate::DecoderStats;

#[derive(Debug, Clone, Default)]
pub(crate) struct StatsHandle(Arc<Mutex<DecoderStats>>);

impl StatsHandle {
    pub(crate) fn update(&self, update: impl FnOnce(&mut DecoderStats)) {
        update(&mut self.0.lock().expect("decoder statistics mutex poisoned"));
    }

    pub(crate) fn snapshot(&self) -> DecoderStats {
        *self.0.lock().expect("decoder statistics mutex poisoned")
    }
}
