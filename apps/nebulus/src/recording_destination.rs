//! Noninteractive recording destinations for native builds.

use std::{
    path::PathBuf,
    sync::atomic::{AtomicU32, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
use std::path::Path;

const RECORDING_DIRECTORY_NAME: &str = "Nebulus";
static RECORDING_SEQUENCE: AtomicU32 = AtomicU32::new(0);

/// Resolve the configured folder and allocate a unique MP4 path without a picker.
pub(crate) fn next_path(configured_directory: &str) -> PathBuf {
    let directory = effective_directory(configured_directory);
    let unix_millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis());
    let sequence = RECORDING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    directory.join(filename(unix_millis, sequence))
}

/// Return the configured folder, or the platform's app-owned default.
pub(crate) fn effective_directory(configured_directory: &str) -> PathBuf {
    let configured = configured_directory.trim();
    if configured.is_empty() {
        default_directory()
    } else {
        PathBuf::from(configured)
    }
}

#[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
fn default_directory() -> PathBuf {
    dirs::video_dir()
        .or_else(dirs::document_dir)
        .or_else(dirs::home_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join(RECORDING_DIRECTORY_NAME)
}

#[cfg(target_os = "android")]
fn default_directory() -> PathBuf {
    crate::android::recordings_directory()
        .unwrap_or_else(|_| std::env::temp_dir().join(RECORDING_DIRECTORY_NAME))
}

fn filename(unix_millis: u128, sequence: u32) -> String {
    let seconds = unix_millis / 1_000;
    let millis = unix_millis % 1_000;
    if sequence == 0 {
        format!("nebulus-{seconds}-{millis:03}.mp4")
    } else {
        format!("nebulus-{seconds}-{millis:03}-{sequence}.mp4")
    }
}

#[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
pub(crate) fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{effective_directory, filename};

    #[test]
    fn recording_names_are_portable_and_collision_safe() {
        assert_eq!(filename(1_717_171_234_567, 0), "nebulus-1717171234-567.mp4");
        assert_eq!(
            filename(1_717_171_234_567, 2),
            "nebulus-1717171234-567-2.mp4"
        );
    }

    #[test]
    fn configured_directory_overrides_the_platform_default() {
        assert_eq!(
            effective_directory("custom-recordings"),
            PathBuf::from("custom-recordings")
        );
    }
}
