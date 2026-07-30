//! Exact-version, checksummed Qubitcoin runtime management.

use crate::config::get_bin_dir;
use crate::process_manager::ServiceId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const RUNTIME_MANIFEST_NAME: &str = "runtime-manifest.json";
const RUNTIME_REPOSITORY: &str = "jonatns/labcoat";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BinaryStatus {
    NotInstalled,
    Installed { version: String },
    Invalid { reason: String },
    Unsupported { platform: String },
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
    #[allow(dead_code)]
    source_digest: String,
    #[allow(dead_code)]
    sources: HashMap<String, RuntimeSource>,
    #[allow(dead_code)]
    compatibility: HashMap<String, String>,
    assets: HashMap<String, RuntimeAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
struct RuntimeSource {
    repository: String,
    revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RuntimeAsset {
    sha256: String,
    size_bytes: u64,
    executable: bool,
    platform: Option<String>,
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

    fn platform() -> String {
        let os = if cfg!(target_os = "macos") {
            "darwin"
        } else if cfg!(target_os = "linux") {
            "linux"
        } else {
            "unknown"
        };
        let arch = if cfg!(target_arch = "aarch64") {
            "arm64"
        } else if cfg!(target_arch = "x86_64") {
            "x86_64"
        } else {
            "unknown"
        };
        format!("{os}-{arch}")
    }

    fn platform_supported(platform: &str) -> bool {
        matches!(platform, "darwin-arm64" | "linux-x86_64")
    }

    fn manifest_path() -> PathBuf {
        get_bin_dir().join(RUNTIME_MANIFEST_NAME)
    }

    fn binary_path(service: ServiceId) -> PathBuf {
        get_bin_dir().join(service.binary_name())
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
        if manifest.schema != 1 {
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
        if manifest.source_digest.len() != 64
            || !manifest
                .source_digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err("Runtime manifest has an invalid source digest".into());
        }
        if manifest.sources.len() != 3
            || manifest.sources.values().any(|source| {
                source.repository.is_empty()
                    || source.revision.len() != 40
                    || !source.revision.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
        {
            return Err("Runtime manifest has invalid source provenance".into());
        }
        if manifest
            .compatibility
            .get("qubitcoin_metashrew_revision")
            .is_none_or(|revision| {
                revision.len() != 40 || !revision.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
        {
            return Err("Runtime manifest has invalid compatibility provenance".into());
        }
        if manifest.assets.is_empty() {
            return Err("Runtime manifest has no assets".into());
        }
        for (name, asset) in &manifest.assets {
            if asset.sha256.len() != 64
                || !asset.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return Err(format!(
                    "Runtime manifest has an invalid checksum for {name}"
                ));
            }
            if asset.size_bytes == 0 {
                return Err(format!("Runtime manifest has an invalid size for {name}"));
            }
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
        Self::write_atomic(&Self::manifest_path(), &bytes, false)?;
        Ok(manifest)
    }

    fn required_assets<'a>(
        manifest: &'a RuntimeManifest,
        platform: &str,
    ) -> Result<Vec<(&'a str, &'a RuntimeAsset, &'static str)>, String> {
        let native_name = format!("qubitcoind-{platform}");
        let native = manifest.assets.get_key_value(&native_name).ok_or_else(|| {
            format!(
                "Runtime {} does not support {platform}",
                Self::release_tag()
            )
        })?;
        if native.1.platform.as_deref() != Some(platform) || !native.1.executable {
            return Err(format!(
                "Runtime manifest has invalid native metadata for {native_name}"
            ));
        }

        let mut assets = vec![(native.0.as_str(), native.1, "qubitcoind")];
        for name in ["alkanes.wasm", "esplorashrew.wasm"] {
            let asset = manifest
                .assets
                .get(name)
                .ok_or_else(|| format!("Runtime manifest is missing {name}"))?;
            if asset.platform.is_some() || asset.executable {
                return Err(format!("Runtime manifest has invalid metadata for {name}"));
            }
            assets.push((name, asset, name));
        }
        Ok(assets)
    }

    fn validate_cached_bundle(manifest: &RuntimeManifest, platform: &str) -> Result<bool, String> {
        Self::validate_bundle_in(&get_bin_dir(), manifest, platform)
    }

    fn validate_bundle_in(
        directory: &Path,
        manifest: &RuntimeManifest,
        platform: &str,
    ) -> Result<bool, String> {
        for (_, asset, local_name) in Self::required_assets(manifest, platform)? {
            let path = directory.join(local_name);
            if !path.exists() {
                return Ok(false);
            }
            let bytes = std::fs::read(&path)
                .map_err(|error| format!("Failed to read {}: {error}", path.display()))?;
            Self::verify_asset(&bytes, asset, path.to_string_lossy().as_ref())?;
        }
        Ok(true)
    }

    pub async fn ensure_bundle(
        progress: impl Fn(ServiceId, f32) + Send + Clone + 'static,
    ) -> Result<(), String> {
        let platform = Self::platform();
        if !Self::platform_supported(&platform) {
            return Err(format!(
                "The managed Labcoat runtime is not available for {platform}; supported platforms are darwin-arm64 and linux-x86_64"
            ));
        }

        if let Ok(Some(manifest)) = Self::load_cached_manifest() {
            if Self::validate_cached_bundle(&manifest, &platform) == Ok(true) {
                return Ok(());
            }
        }

        let manifest = match Self::load_cached_manifest() {
            Ok(Some(manifest)) => manifest,
            Ok(None) | Err(_) => Self::fetch_manifest().await?,
        };
        let base = format!(
            "https://github.com/{RUNTIME_REPOSITORY}/releases/download/{}",
            Self::release_tag()
        );
        for (remote_name, asset, local_name) in Self::required_assets(&manifest, &platform)? {
            let path = get_bin_dir().join(local_name);
            if std::fs::read(&path)
                .ok()
                .is_some_and(|bytes| Self::verify_asset(&bytes, asset, local_name).is_ok())
            {
                continue;
            }
            let callback = progress.clone();
            Self::download_file(
                &format!("{base}/{remote_name}"),
                &path,
                asset,
                move |value| callback(ServiceId::Qubitcoind, value),
            )
            .await?;
        }
        Ok(())
    }

    pub fn check_all(&self) -> Vec<BinaryInfo> {
        ServiceId::all()
            .into_iter()
            .map(|service| self.check_binary(service))
            .collect()
    }

    pub fn check_binary(&self, service: ServiceId) -> BinaryInfo {
        let path = Self::binary_path(service);
        let platform = Self::platform();
        let status = if !Self::platform_supported(&platform) {
            BinaryStatus::Unsupported { platform }
        } else {
            match Self::load_cached_manifest() {
                Ok(Some(manifest)) => {
                    match Self::validate_cached_bundle(&manifest, &Self::platform()) {
                        Ok(true) => BinaryStatus::Installed {
                            version: Self::release_tag(),
                        },
                        Ok(false) => BinaryStatus::NotInstalled,
                        Err(reason) => BinaryStatus::Invalid { reason },
                    }
                }
                Ok(None) => BinaryStatus::NotInstalled,
                Err(reason) => BinaryStatus::Invalid { reason },
            }
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
        Self::write_atomic(path, &bytes, asset.executable)?;
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

    fn write_atomic(path: &Path, bytes: &[u8], executable: bool) -> Result<(), String> {
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
        #[cfg(unix)]
        if executable {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(&temporary)
                .map_err(|error| format!("Failed to inspect {}: {error}", temporary.display()))?
                .permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&temporary, permissions)
                .map_err(|error| format!("Failed to chmod {}: {error}", temporary.display()))?;
        }
        std::fs::rename(&temporary, path).map_err(|error| {
            let _ = std::fs::remove_file(&temporary);
            format!("Failed to install {}: {error}", path.display())
        })?;
        #[cfg(target_os = "macos")]
        if executable {
            let _ = std::process::Command::new("codesign")
                .args(["-s", "-", "-f"])
                .arg(path)
                .status();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(bytes: &[u8], executable: bool, platform: Option<&str>) -> RuntimeAsset {
        RuntimeAsset {
            sha256: hex::encode(Sha256::digest(bytes)),
            size_bytes: bytes.len() as u64,
            executable,
            platform: platform.map(str::to_owned),
        }
    }

    fn manifest(version: &str) -> RuntimeManifest {
        RuntimeManifest {
            schema: 1,
            labcoat_version: version.into(),
            release_tag: format!("cli-v{version}"),
            source_digest: "0".repeat(64),
            sources: HashMap::from([
                (
                    "qubitcoin".into(),
                    RuntimeSource {
                        repository: "example/qubitcoin".into(),
                        revision: "1".repeat(40),
                    },
                ),
                (
                    "alkanes-wasm".into(),
                    RuntimeSource {
                        repository: "example/alkanes".into(),
                        revision: "2".repeat(40),
                    },
                ),
                (
                    "esplorashrew-wasm".into(),
                    RuntimeSource {
                        repository: "example/esplora".into(),
                        revision: "3".repeat(40),
                    },
                ),
            ]),
            compatibility: HashMap::from([("qubitcoin_metashrew_revision".into(), "4".repeat(40))]),
            assets: HashMap::from([
                (
                    "qubitcoind-linux-x86_64".into(),
                    asset(b"native", true, Some("linux-x86_64")),
                ),
                (
                    "qubitcoind-darwin-arm64".into(),
                    asset(b"native", true, Some("darwin-arm64")),
                ),
                ("alkanes.wasm".into(), asset(b"alkanes", false, None)),
                ("esplorashrew.wasm".into(), asset(b"esplora", false, None)),
            ]),
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
    fn runtime_support_is_explicit() {
        assert!(BinaryManager::platform_supported("darwin-arm64"));
        assert!(BinaryManager::platform_supported("linux-x86_64"));
        assert!(!BinaryManager::platform_supported("darwin-x86_64"));
        assert!(!BinaryManager::platform_supported("linux-arm64"));
    }

    #[test]
    fn manifest_must_match_the_exact_cli_release() {
        assert!(BinaryManager::validate_manifest(&manifest(env!("CARGO_PKG_VERSION"))).is_ok());
        let error = BinaryManager::validate_manifest(&manifest("9.9.9")).unwrap_err();
        assert!(error.contains("expected"));
    }

    #[test]
    fn required_assets_map_the_native_binary_to_a_stable_local_name() {
        let fixture = manifest(env!("CARGO_PKG_VERSION"));
        let assets = BinaryManager::required_assets(&fixture, "linux-x86_64").unwrap();
        assert_eq!(assets[0].0, "qubitcoind-linux-x86_64");
        assert_eq!(assets[0].2, "qubitcoind");
        assert_eq!(assets[1].2, "alkanes.wasm");
        assert_eq!(assets[2].2, "esplorashrew.wasm");
        assert!(BinaryManager::required_assets(&fixture, "linux-arm64").is_err());
    }

    #[test]
    fn checksum_and_size_validation_reject_tampering() {
        let fixture = asset(b"runtime", true, Some("linux-x86_64"));
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
    fn incomplete_and_corrupt_cached_bundles_are_not_installed() {
        let root =
            std::env::temp_dir().join(format!("labcoat-runtime-assets-{}", std::process::id()));
        std::fs::remove_dir_all(&root).ok();
        std::fs::create_dir_all(&root).unwrap();
        let fixture = manifest(env!("CARGO_PKG_VERSION"));

        assert!(!BinaryManager::validate_bundle_in(&root, &fixture, "linux-x86_64").unwrap());
        std::fs::write(root.join("qubitcoind"), b"native").unwrap();
        std::fs::write(root.join("alkanes.wasm"), b"alkanes").unwrap();
        assert!(!BinaryManager::validate_bundle_in(&root, &fixture, "linux-x86_64").unwrap());

        std::fs::write(root.join("esplorashrew.wasm"), b"corrupt").unwrap();
        assert!(BinaryManager::validate_bundle_in(&root, &fixture, "linux-x86_64").is_err());
        std::fs::write(root.join("esplorashrew.wasm"), b"esplora").unwrap();
        assert!(BinaryManager::validate_bundle_in(&root, &fixture, "linux-x86_64").unwrap());

        std::fs::remove_dir_all(root).ok();
    }
}
