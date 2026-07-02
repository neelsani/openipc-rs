//! Reusable bounded buffering and timestamp helpers for decoder integrations.

mod clock;
mod mailbox;
mod queue;
#[cfg(any(
    target_os = "android",
    target_os = "macos",
    target_os = "linux",
    target_os = "windows",
    all(target_arch = "wasm32", target_os = "unknown")
))]
mod stats;

pub use clock::RtpTimestampUnwrapper;
pub use mailbox::LatestFrameMailbox;
pub use queue::{BoundedQueue, DropPolicy, QueuePush};
#[cfg(any(
    target_os = "android",
    target_os = "macos",
    target_os = "linux",
    target_os = "windows",
    all(target_arch = "wasm32", target_os = "unknown")
))]
pub(crate) use stats::StatsHandle;
