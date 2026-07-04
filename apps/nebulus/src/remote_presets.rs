//! Bounded remote discovery for data-only preset packs and registries.

use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use url::Url;

use crate::presets::{validate_id, validate_text, PresetPack};

pub(crate) const REGISTRY_SCHEMA_VERSION: u32 = 1;
pub(crate) const DEFAULT_REGISTRY_URL: &str = "https://raw.githubusercontent.com/neelsani/openipc-rs/master/apps/nebulus/presets/registry.json";
pub(crate) const MAX_REGISTRY_BYTES: usize = 512 * 1024;
const MAX_REGISTRY_ENTRIES: usize = 256;
const MAX_REMOTE_URL_LENGTH: usize = 2_048;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RegistryDocument {
    #[serde(rename = "$schema", default, skip_serializing_if = "Option::is_none")]
    json_schema: Option<String>,
    schema_version: u32,
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    homepage: Option<String>,
    presets: Vec<RegistryDocumentEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RegistryDocumentEntry {
    id: String,
    version: String,
    name: String,
    author: String,
    license: String,
    #[serde(default)]
    description: String,
    download_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sha256: Option<String>,
}

/// A validated remote registry ready for display in the preset manager.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PresetRegistry {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) homepage: Option<String>,
    pub(crate) source_url: String,
    pub(crate) presets: Vec<RegistryPreset>,
}

/// One immutable preset version advertised by a registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RegistryPreset {
    pub(crate) id: String,
    pub(crate) version: String,
    pub(crate) name: String,
    pub(crate) author: String,
    pub(crate) license: String,
    pub(crate) description: String,
    pub(crate) download_url: String,
    pub(crate) sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RegistryExpectation {
    id: String,
    version: String,
    sha256: Option<String>,
}

impl RegistryPreset {
    pub(crate) fn request(&self) -> RemoteRequest {
        RemoteRequest {
            url: self.download_url.clone(),
            expected: Some(RegistryExpectation {
                id: self.id.clone(),
                version: self.version.clone(),
                sha256: self.sha256.clone(),
            }),
        }
    }
}

impl PresetRegistry {
    pub(crate) fn parse(bytes: &[u8], source_url: &str) -> Result<Self, String> {
        if bytes.len() > MAX_REGISTRY_BYTES {
            return Err(format!(
                "registry is {} bytes; maximum is {MAX_REGISTRY_BYTES}",
                bytes.len()
            ));
        }
        let document: RegistryDocument = serde_json::from_slice(bytes)
            .map_err(|error| format!("invalid registry JSON: {error}"))?;
        if document.schema_version != REGISTRY_SCHEMA_VERSION {
            return Err(format!(
                "unsupported registry schema {}; this build supports {REGISTRY_SCHEMA_VERSION}",
                document.schema_version
            ));
        }
        if let Some(schema) = document.json_schema.as_deref() {
            validate_text("registry $schema", schema, 256, false)?;
        }
        validate_text("registry name", &document.name, 96, false)?;
        validate_text("registry description", &document.description, 1_024, true)?;
        if document.presets.is_empty() {
            return Err("registry contains no presets".to_owned());
        }
        if document.presets.len() > MAX_REGISTRY_ENTRIES {
            return Err(format!(
                "registry contains more than {MAX_REGISTRY_ENTRIES} entries"
            ));
        }

        let source_url = normalize_remote_url(source_url)?;
        let base =
            Url::parse(&source_url).map_err(|error| format!("invalid registry URL: {error}"))?;
        let homepage = document
            .homepage
            .as_deref()
            .map(|value| resolve_remote_url(&base, value))
            .transpose()?;
        let mut presets = Vec::with_capacity(document.presets.len());
        for entry in document.presets {
            validate_id(&entry.id)?;
            Version::parse(&entry.version)
                .map_err(|error| format!("invalid version for {}: {error}", entry.id))?;
            validate_text("registry preset name", &entry.name, 96, false)?;
            validate_text("registry preset author", &entry.author, 96, false)?;
            validate_text("registry preset license", &entry.license, 64, false)?;
            validate_text(
                "registry preset description",
                &entry.description,
                1_024,
                true,
            )?;
            validate_text(
                "registry preset download URL",
                &entry.download_url,
                MAX_REMOTE_URL_LENGTH,
                false,
            )?;
            let duplicate = presets.iter().any(|existing: &RegistryPreset| {
                existing.id == entry.id && existing.version == entry.version
            });
            if duplicate {
                return Err(format!(
                    "registry repeats preset {} {}",
                    entry.id, entry.version
                ));
            }
            let sha256 = entry.sha256.as_deref().map(normalize_sha256).transpose()?;
            presets.push(RegistryPreset {
                id: entry.id,
                version: entry.version,
                name: entry.name,
                author: entry.author,
                license: entry.license,
                description: entry.description,
                download_url: resolve_remote_url(&base, &entry.download_url)?,
                sha256,
            });
        }
        presets.sort_by(|left, right| {
            left.name
                .to_ascii_lowercase()
                .cmp(&right.name.to_ascii_lowercase())
                .then_with(|| {
                    Version::parse(&right.version)
                        .expect("registry versions were validated")
                        .cmp(
                            &Version::parse(&left.version)
                                .expect("registry versions were validated"),
                        )
                })
        });
        Ok(Self {
            name: document.name,
            description: document.description,
            homepage,
            source_url,
            presets,
        })
    }
}

/// A single bounded network request. With no expectation, its JSON is detected
/// as either a preset pack or registry after download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RemoteRequest {
    pub(crate) url: String,
    expected: Option<RegistryExpectation>,
}

impl RemoteRequest {
    pub(crate) fn direct(url: &str) -> Result<Self, String> {
        Ok(Self {
            url: normalize_remote_url(url)?,
            expected: None,
        })
    }
}

#[derive(Debug, Clone)]
pub(crate) struct RemoteDownload {
    request: RemoteRequest,
    final_url: String,
    bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub(crate) enum RemotePresetContent {
    Preset(PresetPack),
    Registry(PresetRegistry),
}

impl RemoteDownload {
    pub(crate) fn from_parts(
        request: RemoteRequest,
        final_url: String,
        bytes: Vec<u8>,
    ) -> Result<Self, String> {
        if bytes.len() > MAX_REGISTRY_BYTES {
            return Err(format!(
                "remote document exceeds {MAX_REGISTRY_BYTES} bytes"
            ));
        }
        Ok(Self {
            request,
            final_url: normalize_remote_url(&final_url)?,
            bytes,
        })
    }

    pub(crate) fn parse(self) -> Result<RemotePresetContent, String> {
        if let Some(expected) = self.request.expected.as_ref() {
            verify_checksum(&self.bytes, expected.sha256.as_deref())?;
            let pack = PresetPack::parse(&self.bytes)?;
            if pack.id != expected.id || pack.version != expected.version {
                return Err(format!(
                    "registry expected {} {}, but the URL returned {} {}",
                    expected.id, expected.version, pack.id, pack.version
                ));
            }
            return Ok(RemotePresetContent::Preset(pack));
        }

        match PresetPack::parse(&self.bytes) {
            Ok(pack) => Ok(RemotePresetContent::Preset(pack)),
            Err(pack_error) => PresetRegistry::parse(&self.bytes, &self.final_url)
                .map(RemotePresetContent::Registry)
                .map_err(|registry_error| {
                    format!(
                        "URL is neither a valid preset ({pack_error}) nor registry ({registry_error})"
                    )
                }),
        }
    }
}

pub(crate) fn normalize_remote_url(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("enter a preset or registry URL".to_owned());
    }
    if value.len() > MAX_REMOTE_URL_LENGTH {
        return Err(format!(
            "remote URL is longer than {MAX_REMOTE_URL_LENGTH} characters"
        ));
    }
    let mut url = Url::parse(value).map_err(|error| format!("invalid remote URL: {error}"))?;
    if url.host_str() == Some("github.com") {
        let segments = url
            .path_segments()
            .map(|segments| segments.collect::<Vec<_>>())
            .unwrap_or_default();
        if segments.len() >= 5 && segments[2] == "blob" {
            let raw = format!(
                "https://raw.githubusercontent.com/{}/{}/{}/{}",
                segments[0],
                segments[1],
                segments[3],
                segments[4..].join("/")
            );
            url = Url::parse(&raw).expect("generated GitHub raw URL is valid");
        }
    }
    validate_parsed_url(&url)?;
    url.set_fragment(None);
    Ok(url.to_string())
}

fn resolve_remote_url(base: &Url, value: &str) -> Result<String, String> {
    if Url::parse(value).is_ok() {
        return normalize_remote_url(value);
    }
    let url = base
        .join(value)
        .map_err(|error| format!("invalid relative registry URL '{value}': {error}"))?;
    normalize_remote_url(url.as_str())
}

fn validate_parsed_url(url: &Url) -> Result<(), String> {
    if !url.username().is_empty() || url.password().is_some() {
        return Err("remote URLs cannot contain embedded credentials".to_owned());
    }
    let host = url
        .host_str()
        .ok_or_else(|| "remote URL has no host".to_owned())?;
    let loopback = matches!(host, "localhost" | "127.0.0.1" | "::1" | "[::1]");
    if url.scheme() != "https" && !(url.scheme() == "http" && loopback) {
        return Err("remote presets require HTTPS; HTTP is allowed only on loopback".to_owned());
    }
    Ok(())
}

fn normalize_sha256(value: &str) -> Result<String, String> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("registry sha256 must contain exactly 64 hexadecimal characters".to_owned());
    }
    Ok(value.to_ascii_lowercase())
}

fn verify_checksum(bytes: &[u8], expected: Option<&str>) -> Result<(), String> {
    let Some(expected) = expected else {
        return Ok(());
    };
    let actual = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if actual != expected {
        return Err(format!(
            "preset checksum mismatch: expected {expected}, received {actual}"
        ));
    }
    Ok(())
}

#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
pub(crate) fn download(request: RemoteRequest) -> Result<RemoteDownload, String> {
    use std::{io::Read as _, time::Duration};

    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 {
                return attempt.error(std::io::Error::other(
                    "preset request exceeded five redirects",
                ));
            }
            if let Err(error) = validate_parsed_url(attempt.url()) {
                return attempt.error(std::io::Error::other(error));
            }
            attempt.follow()
        }))
        .user_agent(concat!("Nebulus/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| format!("could not initialize HTTPS client: {error}"))?;
    let mut response = client
        .get(&request.url)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|error| format!("remote preset request failed: {error}"))?;
    let final_url = normalize_remote_url(response.url().as_str())?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_REGISTRY_BYTES as u64)
    {
        return Err(format!(
            "remote document exceeds {MAX_REGISTRY_BYTES} bytes"
        ));
    }
    let mut bytes = Vec::with_capacity(response.content_length().unwrap_or_default() as usize);
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let count = response
            .read(&mut buffer)
            .map_err(|error| format!("remote preset download failed: {error}"))?;
        if count == 0 {
            break;
        }
        if bytes.len().saturating_add(count) > MAX_REGISTRY_BYTES {
            return Err(format!(
                "remote document exceeds {MAX_REGISTRY_BYTES} bytes"
            ));
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    RemoteDownload::from_parts(request, final_url, bytes)
}

#[cfg(target_arch = "wasm32")]
pub(crate) async fn download(request: RemoteRequest) -> Result<RemoteDownload, String> {
    use wasm_bindgen::JsCast as _;
    use wasm_bindgen_futures::JsFuture;

    let window = web_sys::window().ok_or_else(|| "browser window is unavailable".to_owned())?;
    let options = web_sys::RequestInit::new();
    options.set_redirect(web_sys::RequestRedirect::Error);
    let browser_request =
        web_sys::Request::new_with_str_and_init(&request.url, &options).map_err(js_error)?;
    let response = JsFuture::from(window.fetch_with_request(&browser_request))
        .await
        .map_err(js_error)?
        .dyn_into::<web_sys::Response>()
        .map_err(|_| "browser returned an invalid fetch response".to_owned())?;
    if !response.ok() {
        return Err(format!(
            "remote preset request returned HTTP {} {}",
            response.status(),
            response.status_text()
        ));
    }
    let final_url = normalize_remote_url(&response.url())?;
    if let Some(length) = response
        .headers()
        .get("content-length")
        .map_err(js_error)?
        .and_then(|length| length.parse::<usize>().ok())
    {
        if length > MAX_REGISTRY_BYTES {
            return Err(format!(
                "remote document exceeds {MAX_REGISTRY_BYTES} bytes"
            ));
        }
    }
    let buffer = JsFuture::from(response.array_buffer().map_err(js_error)?)
        .await
        .map_err(js_error)?;
    let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
    if bytes.len() > MAX_REGISTRY_BYTES {
        return Err(format!(
            "remote document exceeds {MAX_REGISTRY_BYTES} bytes"
        ));
    }
    RemoteDownload::from_parts(request, final_url, bytes)
}

#[cfg(target_arch = "wasm32")]
fn js_error(error: wasm_bindgen::JsValue) -> String {
    error
        .as_string()
        .unwrap_or_else(|| format!("browser remote preset error: {error:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_blob_urls_become_raw_urls() {
        assert_eq!(
            normalize_remote_url(
                "https://github.com/neelsani/openipc-rs/blob/master/apps/nebulus/presets/registry.json"
            )
            .unwrap(),
            DEFAULT_REGISTRY_URL
        );
    }

    #[test]
    fn insecure_and_credentialed_urls_are_rejected() {
        assert!(normalize_remote_url("http://example.com/preset.json").is_err());
        assert!(normalize_remote_url("https://user:secret@example.com/preset.json").is_err());
        assert!(normalize_remote_url("http://127.0.0.1:8080/preset.json").is_ok());
    }

    #[test]
    fn registry_resolves_relative_urls() {
        let registry = br#"{
          "schemaVersion": 1,
          "name": "Test",
          "presets": [{
            "id": "test.pack",
            "version": "1.0.0",
            "name": "Pack",
            "author": "Tester",
            "license": "MIT",
            "downloadUrl": "pack.json"
          }]
        }"#;
        let registry =
            PresetRegistry::parse(registry, "https://example.com/presets/registry.json").unwrap();
        assert_eq!(
            registry.presets[0].download_url,
            "https://example.com/presets/pack.json"
        );
    }

    #[test]
    fn registry_rejects_duplicate_id_versions() {
        let entry = r#"{
          "id": "test.pack",
          "version": "1.0.0",
          "name": "Pack",
          "author": "Tester",
          "license": "MIT",
          "downloadUrl": "pack.json"
        }"#;
        let registry =
            format!("{{\"schemaVersion\":1,\"name\":\"Test\",\"presets\":[{entry},{entry}]}}");
        assert!(
            PresetRegistry::parse(registry.as_bytes(), "https://example.com/registry.json")
                .is_err()
        );
    }

    #[test]
    fn registry_orders_versions_semantically() {
        let registry = br#"{
          "schemaVersion": 1,
          "name": "Test",
          "presets": [
            {
              "id": "test.pack",
              "version": "1.9.0",
              "name": "Pack",
              "author": "Tester",
              "license": "MIT",
              "downloadUrl": "pack-1.9.0.json"
            },
            {
              "id": "test.pack",
              "version": "1.10.0",
              "name": "Pack",
              "author": "Tester",
              "license": "MIT",
              "downloadUrl": "pack-1.10.0.json"
            }
          ]
        }"#;
        let registry =
            PresetRegistry::parse(registry, "https://example.com/presets/registry.json").unwrap();
        assert_eq!(registry.presets[0].version, "1.10.0");
        assert_eq!(registry.presets[1].version, "1.9.0");
    }

    #[test]
    fn checksum_and_registry_identity_are_verified() {
        let pack = PresetPack::parse(include_bytes!(
            "../presets/openipc-standard.nebulus-preset.json"
        ))
        .unwrap();
        let bytes = pack.to_pretty_json().unwrap();
        let checksum = Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let request = RemoteRequest {
            url: "https://example.com/pack.json".to_owned(),
            expected: Some(RegistryExpectation {
                id: pack.id.clone(),
                version: pack.version.clone(),
                sha256: Some(checksum),
            }),
        };
        let content = RemoteDownload {
            request,
            final_url: "https://example.com/pack.json".to_owned(),
            bytes,
        }
        .parse()
        .unwrap();
        assert!(matches!(content, RemotePresetContent::Preset(_)));

        let bad_request = RemoteRequest {
            url: "https://example.com/pack.json".to_owned(),
            expected: Some(RegistryExpectation {
                id: pack.id.clone(),
                version: pack.version.clone(),
                sha256: Some("00".repeat(32)),
            }),
        };
        assert!(RemoteDownload {
            request: bad_request,
            final_url: "https://example.com/pack.json".to_owned(),
            bytes: pack.to_pretty_json().unwrap(),
        }
        .parse()
        .is_err());
    }

    #[test]
    fn bundled_registry_resolves_and_verifies_its_pack() {
        let registry = PresetRegistry::parse(
            include_bytes!("../presets/registry.json"),
            DEFAULT_REGISTRY_URL,
        )
        .unwrap();
        assert_eq!(registry.presets.len(), 1);
        assert_eq!(
            registry.presets[0].download_url,
            "https://raw.githubusercontent.com/neelsani/openipc-rs/master/apps/nebulus/presets/openipc-standard.nebulus-preset.json"
        );
        let content = RemoteDownload {
            request: registry.presets[0].request(),
            final_url: registry.presets[0].download_url.clone(),
            bytes: include_bytes!("../presets/openipc-standard.nebulus-preset.json").to_vec(),
        }
        .parse()
        .unwrap();
        assert!(matches!(content, RemotePresetContent::Preset(_)));
    }

    #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
    #[test]
    fn native_downloader_reads_a_bounded_loopback_document() {
        use std::io::{Read as _, Write as _};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let body = include_bytes!("../presets/openipc-standard.nebulus-preset.json").to_vec();
        let response_body = body.clone();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 2_048];
            let _ = stream.read(&mut request).unwrap();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                response_body.len()
            )
            .unwrap();
            stream.write_all(&response_body).unwrap();
        });

        let request = RemoteRequest::direct(&format!("http://{address}/preset.json")).unwrap();
        let content = download(request).unwrap().parse().unwrap();
        server.join().unwrap();
        let RemotePresetContent::Preset(pack) = content else {
            panic!("expected a preset pack");
        };
        assert_eq!(pack.id, "openipc.standard-fpv");
        assert_eq!(
            body.len(),
            include_bytes!("../presets/openipc-standard.nebulus-preset.json").len()
        );
    }
}
