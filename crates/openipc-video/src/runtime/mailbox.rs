use std::sync::{Arc, Mutex};

/// Single-slot mailbox that always keeps the newest value.
#[derive(Debug)]
pub struct LatestFrameMailbox<T> {
    slot: Arc<Mutex<Option<T>>>,
}

impl<T> Clone for LatestFrameMailbox<T> {
    fn clone(&self) -> Self {
        Self {
            slot: Arc::clone(&self.slot),
        }
    }
}

impl<T> Default for LatestFrameMailbox<T> {
    fn default() -> Self {
        Self {
            slot: Arc::new(Mutex::new(None)),
        }
    }
}

impl<T> LatestFrameMailbox<T> {
    /// Replace the pending value, returning true when an older value was dropped.
    pub fn replace(&self, value: T) -> bool {
        self.slot
            .lock()
            .expect("latest-frame mailbox mutex poisoned")
            .replace(value)
            .is_some()
    }

    /// Take the pending value.
    pub fn take(&self) -> Option<T> {
        self.slot
            .lock()
            .expect("latest-frame mailbox mutex poisoned")
            .take()
    }

    /// Discard any pending value.
    pub fn clear(&self) {
        let _ = self.take();
    }
}
