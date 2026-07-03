//! Windows Wintun discovery and verified first-use installation.

use std::{
    io::{Cursor, Read as _},
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, Sender},
    time::Duration,
};

use sha2::{Digest as _, Sha256};

pub(crate) const VERSION: &str = "0.14.1";
const DOWNLOAD_URL: &str = "https://www.wintun.net/builds/wintun-0.14.1.zip";
const MAX_ARCHIVE_BYTES: usize = 16 * 1024 * 1024;
const EXPECTED_ARCHIVE_SHA256: [u8; 32] = [
    0x07, 0xc2, 0x56, 0x18, 0x5d, 0x6e, 0xe3, 0x65, 0x2e, 0x09, 0xfa, 0x55, 0xc0, 0xb6, 0x73, 0xe2,
    0x62, 0x4b, 0x56, 0x5e, 0x02, 0xc4, 0xb9, 0x09, 0x1c, 0x79, 0xca, 0x7d, 0x2f, 0x24, 0xef, 0x51,
];

#[cfg(target_arch = "x86_64")]
const DLL_ARCHIVE_PATH: &str = "wintun/bin/amd64/wintun.dll";
#[cfg(target_arch = "x86")]
const DLL_ARCHIVE_PATH: &str = "wintun/bin/x86/wintun.dll";
#[cfg(target_arch = "aarch64")]
const DLL_ARCHIVE_PATH: &str = "wintun/bin/arm64/wintun.dll";
#[cfg(target_arch = "arm")]
const DLL_ARCHIVE_PATH: &str = "wintun/bin/arm/wintun.dll";

const LICENSE_ARCHIVE_PATH: &str = "wintun/LICENSE.txt";

#[derive(Debug, Clone)]
pub(crate) enum InstallState {
    Missing,
    Downloading { downloaded: u64, total: Option<u64> },
    Installing,
    Ready,
    Failed(String),
}

impl InstallState {
    pub(crate) fn detect() -> Self {
        locate().map_or(Self::Missing, |_| Self::Ready)
    }

    pub(crate) const fn is_ready(&self) -> bool {
        matches!(self, Self::Ready)
    }
}

#[derive(Debug)]
pub(crate) enum InstallEvent {
    Progress { downloaded: u64, total: Option<u64> },
    Installing,
    Complete(PathBuf),
    Failed(String),
}

pub(crate) fn spawn_installer(
    context: eframe::egui::Context,
) -> Result<Receiver<InstallEvent>, String> {
    let (sender, receiver) = mpsc::channel();
    std::thread::Builder::new()
        .name("nebulus-wintun-installer".to_owned())
        .spawn(move || match install(&sender, &context) {
            Ok(path) => emit(&sender, &context, InstallEvent::Complete(path)),
            Err(error) => emit(&sender, &context, InstallEvent::Failed(error)),
        })
        .map_err(|error| format!("could not start Wintun installer: {error}"))?;
    Ok(receiver)
}

/// Find an existing Wintun DLL and return an absolute path for `LoadLibraryEx`.
pub(crate) fn locate() -> Option<PathBuf> {
    candidate_paths()
        .into_iter()
        .find(|path| path.is_file())
        .map(absolute_path)
}

fn candidate_paths() -> Vec<PathBuf> {
    let mut candidates = Vec::with_capacity(3);
    if let Some(path) = std::env::var_os("NEBULUS_WINTUN_DLL") {
        candidates.push(PathBuf::from(path));
    }
    if let Ok(executable) = std::env::current_exe() {
        if let Some(directory) = executable.parent() {
            candidates.push(directory.join("wintun.dll"));
        }
    }
    if let Ok(path) = installed_dll_path() {
        candidates.push(path);
    }
    candidates
}

fn install(
    sender: &Sender<InstallEvent>,
    context: &eframe::egui::Context,
) -> Result<PathBuf, String> {
    if let Some(path) = locate() {
        return Ok(path);
    }

    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(60))
        .user_agent(concat!("Nebulus/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| format!("could not initialize HTTPS client: {error}"))?;
    let mut response = client
        .get(DOWNLOAD_URL)
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|error| format!("Wintun download failed: {error}"))?;
    let total = response.content_length();
    if total.is_some_and(|length| length > MAX_ARCHIVE_BYTES as u64) {
        return Err("Wintun download exceeded the expected size".to_owned());
    }

    let mut bytes = Vec::with_capacity(total.unwrap_or_default() as usize);
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = response
            .read(&mut buffer)
            .map_err(|error| format!("could not read Wintun download: {error}"))?;
        if count == 0 {
            break;
        }
        if bytes.len().saturating_add(count) > MAX_ARCHIVE_BYTES {
            return Err("Wintun download exceeded the expected size".to_owned());
        }
        bytes.extend_from_slice(&buffer[..count]);
        emit(
            sender,
            context,
            InstallEvent::Progress {
                downloaded: bytes.len() as u64,
                total,
            },
        );
    }

    emit(sender, context, InstallEvent::Installing);
    let digest = Sha256::digest(&bytes);
    if digest[..] != EXPECTED_ARCHIVE_SHA256 {
        return Err("Wintun archive failed SHA-256 verification".to_owned());
    }

    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| format!("could not open Wintun archive: {error}"))?;
    let dll = read_archive_entry(&mut archive, DLL_ARCHIVE_PATH)?;
    let license = read_archive_entry(&mut archive, LICENSE_ARCHIVE_PATH)?;
    let destination = installed_dll_path()?;
    let directory = destination
        .parent()
        .ok_or_else(|| "Wintun install path has no parent directory".to_owned())?;
    std::fs::create_dir_all(directory)
        .map_err(|error| format!("could not create Wintun directory: {error}"))?;
    atomic_write(&directory.join("LICENSE-WINTUN.txt"), &license)?;
    atomic_write(&destination, &dll)?;
    Ok(absolute_path(destination))
}

fn read_archive_entry(
    archive: &mut zip::ZipArchive<Cursor<Vec<u8>>>,
    name: &str,
) -> Result<Vec<u8>, String> {
    let mut entry = archive
        .by_name(name)
        .map_err(|error| format!("Wintun archive is missing {name}: {error}"))?;
    if entry.size() == 0 || entry.size() > MAX_ARCHIVE_BYTES as u64 {
        return Err(format!("Wintun archive contains an invalid {name}"));
    }
    let capacity = usize::try_from(entry.size()).unwrap_or(MAX_ARCHIVE_BYTES);
    let mut bytes = Vec::with_capacity(capacity);
    entry
        .read_to_end(&mut bytes)
        .map_err(|error| format!("could not extract {name}: {error}"))?;
    if bytes.is_empty() || bytes.len() > MAX_ARCHIVE_BYTES {
        return Err(format!("Wintun archive contains an invalid {name}"));
    }
    Ok(bytes)
}

fn installed_dll_path() -> Result<PathBuf, String> {
    let local_app_data = std::env::var_os("LOCALAPPDATA")
        .filter(|path| !path.is_empty())
        .ok_or_else(|| "Windows LOCALAPPDATA is unavailable".to_owned())?;
    Ok(PathBuf::from(local_app_data)
        .join("Nebulus")
        .join("wintun")
        .join(VERSION)
        .join("wintun.dll"))
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut temporary_name = path.as_os_str().to_owned();
    temporary_name.push(".tmp");
    let temporary = PathBuf::from(temporary_name);
    std::fs::write(&temporary, bytes)
        .map_err(|error| format!("could not write {}: {error}", path.display()))?;
    if path.exists() {
        std::fs::remove_file(path)
            .map_err(|error| format!("could not replace {}: {error}", path.display()))?;
    }
    std::fs::rename(&temporary, path)
        .map_err(|error| format!("could not install {}: {error}", path.display()))
}

fn absolute_path(path: PathBuf) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| {
        if path.is_absolute() {
            path.clone()
        } else {
            std::env::current_dir().map_or(path.clone(), |directory| directory.join(path))
        }
    })
}

fn emit(sender: &Sender<InstallEvent>, context: &eframe::egui::Context, event: InstallEvent) {
    let _ = sender.send(event);
    context.request_repaint();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_archive_hash_has_sha256_length() {
        assert_eq!(EXPECTED_ARCHIVE_SHA256.len(), 32);
    }

    #[test]
    fn target_archive_entry_is_arch_specific() {
        assert!(DLL_ARCHIVE_PATH.starts_with("wintun/bin/"));
        assert!(DLL_ARCHIVE_PATH.ends_with("/wintun.dll"));
    }
}
