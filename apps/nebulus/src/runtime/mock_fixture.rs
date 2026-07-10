//! Development codec-fixture discovery, integrity checking, and caching.

use openipc_core::rtp::Codec;
use sha2::{Digest as _, Sha256};

use super::codec_mock::MockVideoConfig;

const MAX_FIXTURE_BYTES: usize = 24 * 1024 * 1024;
const RAW_REPOSITORY: &str = "https://raw.githubusercontent.com/neelsani/openipc-rs";

pub(crate) struct LoadedMockFixture {
    pub(crate) bytes: Vec<u8>,
    pub(crate) origin: String,
}

fn download_urls(file_name: &str) -> [String; 2] {
    [
        format!(
            "{RAW_REPOSITORY}/v{}/apps/nebulus/assets/{file_name}",
            env!("CARGO_PKG_VERSION")
        ),
        format!("{RAW_REPOSITORY}/master/apps/nebulus/assets/{file_name}"),
    ]
}

fn validate_fixture(bytes: &[u8], config: MockVideoConfig, codec: Codec) -> Result<(), String> {
    if bytes.is_empty() {
        return Err("downloaded fixture is empty".to_owned());
    }
    if bytes.len() > MAX_FIXTURE_BYTES {
        return Err(format!(
            "fixture is {} bytes; limit is {MAX_FIXTURE_BYTES} bytes",
            bytes.len()
        ));
    }
    let digest = Sha256::digest(bytes);
    let actual = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let expected = config.resolution.fixture_sha256(codec);
    if actual != expected {
        return Err(format!(
            "fixture checksum mismatch: expected {expected}, received {actual}"
        ));
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn load_sync(
    config: MockVideoConfig,
    codec: Codec,
) -> Result<LoadedMockFixture, String> {
    use std::{fs, path::Path};

    let file_name = config.resolution.fixture_name(codec);
    let source_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join(file_name);
    if source_path.is_file() {
        let bytes = fs::read(&source_path).map_err(|error| {
            format!(
                "could not read local mock fixture {}: {error}",
                source_path.display()
            )
        })?;
        validate_fixture(&bytes, config, codec).map_err(|error| {
            format!(
                "local mock fixture {} is invalid: {error}",
                source_path.display()
            )
        })?;
        return Ok(LoadedMockFixture {
            bytes,
            origin: format!("local source fixture {}", source_path.display()),
        });
    }

    let cache_path = cache_directory()?.join(file_name);
    if cache_path.is_file() {
        match fs::read(&cache_path) {
            Ok(bytes) if validate_fixture(&bytes, config, codec).is_ok() => {
                return Ok(LoadedMockFixture {
                    bytes,
                    origin: format!("cached fixture {}", cache_path.display()),
                });
            }
            _ => {
                let _ = fs::remove_file(&cache_path);
            }
        }
    }

    let mut failures = Vec::new();
    for url in download_urls(file_name) {
        match download_native(&url) {
            Ok(bytes) => {
                if let Err(error) = validate_fixture(&bytes, config, codec) {
                    failures.push(format!("{url}: {error}"));
                    continue;
                }
                let cache_note = match store_cache(&cache_path, &bytes) {
                    Ok(()) => format!("cached at {}", cache_path.display()),
                    Err(error) => format!("cache unavailable: {error}"),
                };
                return Ok(LoadedMockFixture {
                    bytes,
                    origin: format!("downloaded {url}; {cache_note}"),
                });
            }
            Err(error) => failures.push(format!("{url}: {error}")),
        }
    }

    Err(format!(
        "could not load development codec fixture {file_name}; {}",
        failures.join("; ")
    ))
}

#[cfg(not(target_arch = "wasm32"))]
fn store_cache(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    use std::fs;

    let parent = path
        .parent()
        .ok_or_else(|| "mock cache path has no parent".to_owned())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
    let temporary = path.with_extension(format!(
        "{}.part-{}",
        path.extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("fixture"),
        std::process::id()
    ));
    fs::write(&temporary, bytes)
        .map_err(|error| format!("could not write {}: {error}", temporary.display()))?;
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(format!("could not finalize {}: {error}", path.display()));
    }
    Ok(())
}

#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
fn cache_directory() -> Result<std::path::PathBuf, String> {
    dirs::cache_dir()
        .map(|root| root.join("openipc-rs").join("nebulus").join("mock"))
        .ok_or_else(|| "operating-system cache directory is unavailable".to_owned())
}

#[cfg(target_os = "android")]
fn cache_directory() -> Result<std::path::PathBuf, String> {
    crate::android::mock_fixture_directory()
}

#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
fn download_native(url: &str) -> Result<Vec<u8>, String> {
    use std::time::Duration;

    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|error| format!("could not create HTTP client: {error}"))?;
    let response = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/octet-stream")
        .header(reqwest::header::USER_AGENT, "Nebulus codec fixture loader")
        .send()
        .map_err(|error| format!("request failed: {error}"))?;
    if !response.status().is_success() {
        return Err(format!("request returned HTTP {}", response.status()));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_FIXTURE_BYTES as u64)
    {
        return Err(format!(
            "server advertised a body larger than {MAX_FIXTURE_BYTES} bytes"
        ));
    }
    let bytes = response
        .bytes()
        .map_err(|error| format!("could not read response body: {error}"))?;
    if bytes.len() > MAX_FIXTURE_BYTES {
        return Err(format!(
            "response exceeded the {MAX_FIXTURE_BYTES}-byte limit"
        ));
    }
    Ok(bytes.to_vec())
}

#[cfg(target_os = "android")]
fn download_native(url: &str) -> Result<Vec<u8>, String> {
    crate::android::download_mock_fixture(url, MAX_FIXTURE_BYTES)
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub(crate) async fn load(
    config: MockVideoConfig,
    codec: Codec,
) -> Result<LoadedMockFixture, String> {
    use js_sys::Uint8Array;
    use wasm_bindgen::JsCast as _;
    use wasm_bindgen_futures::JsFuture;
    use web_sys::Response;

    let file_name = config.resolution.fixture_name(codec);
    let window = web_sys::window().ok_or_else(|| "browser Window is unavailable".to_owned())?;
    let mut failures = Vec::new();
    for url in download_urls(file_name) {
        let response = match JsFuture::from(window.fetch_with_str(&url)).await {
            Ok(value) => value
                .dyn_into::<Response>()
                .map_err(|_| "fetch returned a non-Response value".to_owned()),
            Err(error) => Err(format!("fetch failed: {}", js_error(&error))),
        };
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                failures.push(format!("{url}: {error}"));
                continue;
            }
        };
        if !response.ok() {
            failures.push(format!(
                "{url}: request returned HTTP {}",
                response.status()
            ));
            continue;
        }
        if response
            .headers()
            .get("content-length")
            .ok()
            .flatten()
            .and_then(|length| length.parse::<usize>().ok())
            .is_some_and(|length| length > MAX_FIXTURE_BYTES)
        {
            failures.push(format!(
                "{url}: server advertised a body larger than {MAX_FIXTURE_BYTES} bytes"
            ));
            continue;
        }
        let buffer = match response.array_buffer() {
            Ok(promise) => match JsFuture::from(promise).await {
                Ok(buffer) => buffer,
                Err(error) => {
                    failures.push(format!(
                        "{url}: could not read response body: {}",
                        js_error(&error)
                    ));
                    continue;
                }
            },
            Err(error) => {
                failures.push(format!(
                    "{url}: could not open response body: {}",
                    js_error(&error)
                ));
                continue;
            }
        };
        let bytes = Uint8Array::new(&buffer).to_vec();
        if let Err(error) = validate_fixture(&bytes, config, codec) {
            failures.push(format!("{url}: {error}"));
            continue;
        }
        return Ok(LoadedMockFixture {
            bytes,
            origin: format!("downloaded {url} using the browser HTTP cache"),
        });
    }

    Err(format!(
        "could not load development codec fixture {file_name}; {}",
        failures.join("; ")
    ))
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn js_error(error: &wasm_bindgen::JsValue) -> String {
    error.as_string().unwrap_or_else(|| format!("{error:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::codec_mock::MockResolution;

    #[test]
    fn repository_fixtures_match_pinned_checksums() {
        for resolution in MockResolution::ALL {
            for codec in [Codec::H264, Codec::H265] {
                let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("assets")
                    .join(resolution.fixture_name(codec));
                let bytes = std::fs::read(&path).unwrap();
                let config = MockVideoConfig {
                    resolution,
                    fps: 60,
                };
                validate_fixture(&bytes, config, codec).unwrap();
            }
        }
    }

    #[test]
    fn altered_fixture_is_rejected() {
        let config = MockVideoConfig::default();
        let mut bytes = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("assets")
                .join(config.resolution.fixture_name(Codec::H264)),
        )
        .unwrap();
        bytes[0] ^= 1;
        assert!(validate_fixture(&bytes, config, Codec::H264)
            .unwrap_err()
            .contains("checksum mismatch"));
    }
}
