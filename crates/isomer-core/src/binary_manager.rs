//! The exact-version, checksummed runtime asset Labcoat Network needs.
//!
//! Labcoat Network runs inside the `labcoat` executable, so its only runtime
//! asset is the Alkanes indexer module, `alkanes.wasm`. Each CLI release
//! publishes it beside a `runtime-manifest.json` that pins its source and
//! checksum. A CLI only ever reads the manifest of its own release.

use crate::config::get_bin_dir;
use crate::process_manager::{ServiceId, INDEXER_ASSET};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const RUNTIME_MANIFEST_NAME: &str = "runtime-manifest.json";
const RUNTIME_REPOSITORY: &str = "jonatns/labcoat";
/// The manifest version this build reads. Version 2 lists only the indexer
/// module; version 1 also listed Qubitcoin and Esplorashrew.
const MANIFEST_SCHEMA: u32 = 2;
/// The source the indexer module is built from, as named in the manifest.
const INDEXER_SOURCE: &str = "alkanes-wasm";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BinaryStatus {
    NotInstalled,
    Installed { version: String },
    Invalid { reason: String },
}

impl BinaryStatus {
    pub fn into_version(self) -> Option<String> {
        match self {
            Self::Installed { version } => Some(version),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryInfo {
    pub service: String,
    pub status: BinaryStatus,
    pub path: String,
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RuntimeManifest {
    schema: u32,
    labcoat_version: String,
    release_tag: String,
    source_digest: String,
    sources: HashMap<String, RuntimeSource>,
    assets: HashMap<String, RuntimeAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RuntimeSource {
    repository: String,
    revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RuntimeAsset {
    sha256: String,
    size_bytes: u64,
}

pub struct BinaryManager;

impl Default for BinaryManager {
    fn default() -> Self {
        Self::new()
    }
}

impl BinaryManager {
    pub fn new() -> Self {
        Self
    }

    pub fn release_tag() -> String {
        format!("cli-v{}", env!("CARGO_PKG_VERSION"))
    }

    fn manifest_url() -> String {
        format!(
            "https://github.com/{RUNTIME_REPOSITORY}/releases/download/{}/{}",
            Self::release_tag(),
            RUNTIME_MANIFEST_NAME
        )
    }

    fn manifest_path() -> PathBuf {
        get_bin_dir().join(RUNTIME_MANIFEST_NAME)
    }

    fn asset_path() -> PathBuf {
        get_bin_dir().join(INDEXER_ASSET)
    }

    fn load_cached_manifest() -> Result<Option<RuntimeManifest>, String> {
        Self::load_cached_manifest_from(&Self::manifest_path())
    }

    fn load_cached_manifest_from(path: &Path) -> Result<Option<RuntimeManifest>, String> {
        if !path.exists() {
            return Ok(None);
        }
        let source = std::fs::read_to_string(path)
            .map_err(|error| format!("Failed to read {}: {error}", path.display()))?;
        let manifest = serde_json::from_str(&source)
            .map_err(|error| format!("Invalid cached runtime manifest: {error}"))?;
        Self::validate_manifest(&manifest)?;
        Ok(Some(manifest))
    }

    fn validate_manifest(manifest: &RuntimeManifest) -> Result<(), String> {
        if manifest.schema != MANIFEST_SCHEMA {
            return Err(format!(
                "Unsupported runtime manifest schema {}",
                manifest.schema
            ));
        }
        if manifest.labcoat_version != env!("CARGO_PKG_VERSION") {
            return Err(format!(
                "Runtime manifest is for Labcoat {}, expected {}",
                manifest.labcoat_version,
                env!("CARGO_PKG_VERSION")
            ));
        }
        if manifest.release_tag != Self::release_tag() {
            return Err(format!(
                "Runtime manifest tag {} does not match {}",
                manifest.release_tag,
                Self::release_tag()
            ));
        }
        if !is_hex(&manifest.source_digest, 64) {
            return Err("Runtime manifest has an invalid source digest".into());
        }
        let source = manifest
            .sources
            .get(INDEXER_SOURCE)
            .filter(|source| !source.repository.is_empty() && is_hex(&source.revision, 40));
        if manifest.sources.len() != 1 || source.is_none() {
            return Err("Runtime manifest has invalid source provenance".into());
        }
        let asset = manifest
            .assets
            .get(INDEXER_ASSET)
            .ok_or_else(|| format!("Runtime manifest is missing {INDEXER_ASSET}"))?;
        if manifest.assets.len() != 1 || !is_hex(&asset.sha256, 64) || asset.size_bytes == 0 {
            return Err(format!(
                "Runtime manifest has invalid metadata for {INDEXER_ASSET}"
            ));
        }
        Ok(())
    }

    async fn fetch_manifest() -> Result<RuntimeManifest, String> {
        let response = reqwest::get(Self::manifest_url())
            .await
            .map_err(|error| format!("Failed to fetch runtime manifest: {error}"))?;
        if !response.status().is_success() {
            return Err(format!(
                "Failed to fetch runtime manifest for {}: {}",
                Self::release_tag(),
                response.status()
            ));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|error| format!("Failed to read runtime manifest: {error}"))?;
        let manifest: RuntimeManifest = serde_json::from_slice(&bytes)
            .map_err(|error| format!("Invalid runtime manifest: {error}"))?;
        Self::validate_manifest(&manifest)?;
        Self::write_atomic(&Self::manifest_path(), &bytes)?;
        Ok(manifest)
    }

    fn asset(manifest: &RuntimeManifest) -> &RuntimeAsset {
        manifest
            .assets
            .get(INDEXER_ASSET)
            .expect("a validated manifest lists the indexer")
    }

    fn validate_bundle_in(directory: &Path, manifest: &RuntimeManifest) -> Result<bool, String> {
        let path = directory.join(INDEXER_ASSET);
        if !path.exists() {
            return Ok(false);
        }
        let bytes = std::fs::read(&path)
            .map_err(|error| format!("Failed to read {}: {error}", path.display()))?;
        Self::verify_asset(
            &bytes,
            Self::asset(manifest),
            path.to_string_lossy().as_ref(),
        )?;
        Ok(true)
    }

    /// Make sure this release's indexer module is installed and intact,
    /// downloading it if not.
    pub async fn ensure_bundle(
        progress: impl Fn(ServiceId, f32) + Send + Clone + 'static,
    ) -> Result<(), String> {
        if let Ok(Some(manifest)) = Self::load_cached_manifest() {
            if Self::validate_bundle_in(&get_bin_dir(), &manifest) == Ok(true) {
                return Ok(());
            }
        }

        let manifest = match Self::load_cached_manifest() {
            Ok(Some(manifest)) => manifest,
            Ok(None) | Err(_) => Self::fetch_manifest().await?,
        };
        let url = format!(
            "https://github.com/{RUNTIME_REPOSITORY}/releases/download/{}/{INDEXER_ASSET}",
            Self::release_tag()
        );
        Self::download_file(
            &url,
            &Self::asset_path(),
            Self::asset(&manifest),
            move |value| progress(ServiceId::Node, value),
        )
        .await
    }

    pub fn check_all(&self) -> Vec<BinaryInfo> {
        ServiceId::all()
            .into_iter()
            .map(|service| self.check_binary(service))
            .collect()
    }

    pub fn check_binary(&self, service: ServiceId) -> BinaryInfo {
        let path = Self::asset_path();
        let status = match Self::load_cached_manifest() {
            Ok(Some(manifest)) => match Self::validate_bundle_in(&get_bin_dir(), &manifest) {
                Ok(true) => BinaryStatus::Installed {
                    version: Self::release_tag(),
                },
                Ok(false) => BinaryStatus::NotInstalled,
                Err(reason) => BinaryStatus::Invalid { reason },
            },
            Ok(None) => BinaryStatus::NotInstalled,
            Err(reason) => BinaryStatus::Invalid { reason },
        };
        BinaryInfo {
            service: service.display_name().into(),
            status,
            path: path.display().to_string(),
            size_bytes: std::fs::metadata(&path).ok().map(|metadata| metadata.len()),
        }
    }

    async fn download_file(
        url: &str,
        path: &Path,
        asset: &RuntimeAsset,
        progress: impl Fn(f32),
    ) -> Result<(), String> {
        progress(0.0);
        let response = reqwest::get(url)
            .await
            .map_err(|error| format!("Download failed for {url}: {error}"))?;
        if !response.status().is_success() {
            return Err(format!(
                "Download failed for {url} with status {}",
                response.status()
            ));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|error| format!("Failed to read {url}: {error}"))?;
        progress(0.9);
        Self::verify_asset(&bytes, asset, url)?;
        Self::write_atomic(path, &bytes)?;
        progress(1.0);
        Ok(())
    }

    fn verify_asset(bytes: &[u8], asset: &RuntimeAsset, name: &str) -> Result<(), String> {
        if bytes.len() as u64 != asset.size_bytes {
            return Err(format!(
                "Runtime size mismatch for {name}: expected {}, got {}",
                asset.size_bytes,
                bytes.len()
            ));
        }
        let actual = hex::encode(Sha256::digest(bytes));
        if actual != asset.sha256.to_ascii_lowercase() {
            return Err(format!(
                "Checksum verification failed for {name}: expected {}, got {actual}",
                asset.sha256
            ));
        }
        Ok(())
    }

    fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
        let parent = path
            .parent()
            .ok_or_else(|| format!("Runtime path has no parent: {}", path.display()))?;
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("Failed to create {}: {error}", parent.display()))?;
        let filename = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| format!("Invalid runtime path: {}", path.display()))?;
        let temporary = parent.join(format!(".{filename}.partial-{}", std::process::id()));
        std::fs::write(&temporary, bytes)
            .map_err(|error| format!("Failed to write {}: {error}", temporary.display()))?;
        std::fs::rename(&temporary, path).map_err(|error| {
            let _ = std::fs::remove_file(&temporary);
            format!("Failed to install {}: {error}", path.display())
        })
    }
}

fn is_hex(value: &str, length: usize) -> bool {
    value.len() == length && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(bytes: &[u8]) -> RuntimeAsset {
        RuntimeAsset {
            sha256: hex::encode(Sha256::digest(bytes)),
            size_bytes: bytes.len() as u64,
        }
    }

    fn manifest(version: &str) -> RuntimeManifest {
        RuntimeManifest {
            schema: MANIFEST_SCHEMA,
            labcoat_version: version.into(),
            release_tag: format!("cli-v{version}"),
            source_digest: "0".repeat(64),
            sources: HashMap::from([(
                INDEXER_SOURCE.into(),
                RuntimeSource {
                    repository: "example/alkanes".into(),
                    revision: "2".repeat(40),
                },
            )]),
            assets: HashMap::from([(INDEXER_ASSET.into(), asset(b"alkanes"))]),
        }
    }

    #[test]
    fn release_manifest_url_is_derived_from_the_cli_version() {
        assert_eq!(
            BinaryManager::manifest_url(),
            format!(
                "https://github.com/jonatns/labcoat/releases/download/cli-v{0}/runtime-manifest.json",
                env!("CARGO_PKG_VERSION")
            )
        );
    }

    #[test]
    fn manifest_must_match_the_exact_cli_release() {
        assert!(BinaryManager::validate_manifest(&manifest(env!("CARGO_PKG_VERSION"))).is_ok());
        let error = BinaryManager::validate_manifest(&manifest("9.9.9")).unwrap_err();
        assert!(error.contains("expected"));
    }

    #[test]
    fn manifest_must_list_only_the_indexer() {
        let mut fixture = manifest(env!("CARGO_PKG_VERSION"));
        fixture
            .assets
            .insert("qubitcoind-linux-x86_64".into(), asset(b"native"));
        assert!(BinaryManager::validate_manifest(&fixture).is_err());

        let mut fixture = manifest(env!("CARGO_PKG_VERSION"));
        fixture.schema = 1;
        assert!(BinaryManager::validate_manifest(&fixture)
            .unwrap_err()
            .contains("schema 1"));
    }

    #[test]
    fn checksum_and_size_validation_reject_tampering() {
        let fixture = asset(b"runtime");
        assert!(BinaryManager::verify_asset(b"runtime", &fixture, "fixture").is_ok());
        assert!(BinaryManager::verify_asset(b"tampered", &fixture, "fixture").is_err());
        assert!(BinaryManager::verify_asset(b"short", &fixture, "fixture").is_err());
    }

    #[test]
    fn cached_manifest_is_selected_before_any_fetch_is_needed() {
        let root =
            std::env::temp_dir().join(format!("labcoat-runtime-cache-{}", std::process::id()));
        std::fs::remove_dir_all(&root).ok();
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join(RUNTIME_MANIFEST_NAME);
        let fixture = manifest(env!("CARGO_PKG_VERSION"));
        std::fs::write(&path, serde_json::to_vec(&fixture).unwrap()).unwrap();
        assert!(BinaryManager::load_cached_manifest_from(&path)
            .unwrap()
            .is_some());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn missing_and_corrupt_modules_are_not_installed() {
        let root =
            std::env::temp_dir().join(format!("labcoat-runtime-assets-{}", std::process::id()));
        std::fs::remove_dir_all(&root).ok();
        std::fs::create_dir_all(&root).unwrap();
        let fixture = manifest(env!("CARGO_PKG_VERSION"));

        assert!(!BinaryManager::validate_bundle_in(&root, &fixture).unwrap());
        std::fs::write(root.join(INDEXER_ASSET), b"corrupt").unwrap();
        assert!(BinaryManager::validate_bundle_in(&root, &fixture).is_err());
        std::fs::write(root.join(INDEXER_ASSET), b"alkanes").unwrap();
        assert!(BinaryManager::validate_bundle_in(&root, &fixture).unwrap());

        std::fs::remove_dir_all(root).ok();
    }
}
