//! Checksummed Qubitcoin runtime download management.

use crate::config::get_bin_dir;
use crate::process_manager::ServiceId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BinaryStatus {
    NotInstalled,
    Downloading { progress: f32 },
    Installed { version: String },
    UpdateAvailable { current: String, latest: String },
}

impl BinaryStatus {
    pub fn into_version(self) -> Option<String> {
        match self {
            Self::Installed { version } => Some(version),
            Self::UpdateAvailable { current, .. } => Some(current),
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

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct BinaryRelease {
    version: String,
    url: String,
    sha256: String,
    size_bytes: u64,
}

pub struct BinaryManager {
    releases: HashMap<ServiceId, BinaryRelease>,
    checksums_cache: Option<HashMap<String, String>>,
    manifest: RuntimeManifest,
}

const RUNTIME_MANIFEST: &str = include_str!("../../../runtime.json");

#[derive(Debug, Clone, Deserialize)]
struct RuntimeManifest {
    schema: u32,
    active_release: ActiveRelease,
    #[allow(dead_code)]
    sources: HashMap<String, RuntimeSource>,
    hosted: HashMap<String, HostedComponent>,
}

#[derive(Debug, Clone, Deserialize)]
struct ActiveRelease {
    owner: String,
    repository: String,
    tag: String,
    checksums_asset: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct RuntimeSource {
    repository: String,
    revision: String,
    version: String,
}

#[derive(Debug, Clone, Deserialize)]
struct HostedComponent {
    version: String,
    asset_pattern: String,
    size_bytes: u64,
    sha256: HashMap<String, String>,
}

impl RuntimeManifest {
    fn load() -> Self {
        let manifest: Self =
            serde_json::from_str(RUNTIME_MANIFEST).expect("embedded runtime.json must be valid");
        assert_eq!(manifest.schema, 2, "unsupported runtime.json schema");
        manifest
    }

    fn release_base(&self) -> String {
        format!(
            "https://github.com/{}/{}/releases/download/{}",
            self.active_release.owner, self.active_release.repository, self.active_release.tag
        )
    }
}

impl Default for BinaryManager {
    fn default() -> Self {
        Self::new()
    }
}

impl BinaryManager {
    pub fn new() -> Self {
        let manifest = RuntimeManifest::load();
        Self {
            releases: Self::get_releases_for_platform(&manifest),
            checksums_cache: None,
            manifest,
        }
    }

    pub async fn fetch_checksums(&mut self) -> Result<(), String> {
        if self.checksums_cache.is_some() {
            return Ok(());
        }
        let Some(asset) = self.manifest.active_release.checksums_asset.as_ref() else {
            return Ok(());
        };
        let response = reqwest::get(format!("{}/{}", self.manifest.release_base(), asset))
            .await
            .map_err(|e| format!("Failed to fetch checksums: {e}"))?;
        if !response.status().is_success() {
            return Err(format!(
                "Failed to fetch runtime checksums: {}",
                response.status()
            ));
        }
        self.checksums_cache = Some(
            response
                .json()
                .await
                .map_err(|e| format!("Failed to parse checksums: {e}"))?,
        );
        Ok(())
    }

    fn platform() -> (&'static str, &'static str) {
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
        (os, arch)
    }

    fn get_releases_for_platform(manifest: &RuntimeManifest) -> HashMap<ServiceId, BinaryRelease> {
        let (os, arch) = Self::platform();
        Self::get_releases_for(manifest, os, arch)
    }

    fn get_releases_for(
        manifest: &RuntimeManifest,
        os: &str,
        arch: &str,
    ) -> HashMap<ServiceId, BinaryRelease> {
        let mut releases = HashMap::new();
        let platform = format!("{os}-{arch}");
        let Some(component) = manifest.hosted.get("qubitcoind") else {
            return releases;
        };
        let Some(checksum) = component.sha256.get(&platform) else {
            return releases;
        };
        let asset = component.asset_pattern.replace("{platform}", &platform);
        releases.insert(
            ServiceId::Qubitcoind,
            BinaryRelease {
                version: component.version.clone(),
                url: format!("{}/{}", manifest.release_base(), asset),
                sha256: checksum.clone(),
                size_bytes: component.size_bytes,
            },
        );
        releases
    }

    fn binary_path(service: ServiceId) -> PathBuf {
        get_bin_dir().join(service.binary_name())
    }

    pub fn is_installed(service: ServiceId) -> bool {
        Self::binary_path(service).exists()
    }

    pub fn check_all(&self) -> Vec<BinaryInfo> {
        ServiceId::all()
            .into_iter()
            .map(|service| self.check_binary(service))
            .collect()
    }

    pub fn check_binary(&self, service: ServiceId) -> BinaryInfo {
        let path = Self::binary_path(service);
        let exists = path.exists();
        let status = if exists {
            BinaryStatus::Installed {
                version: self
                    .run_version_cmd(&path)
                    .unwrap_or_else(|| "unknown".into()),
            }
        } else {
            BinaryStatus::NotInstalled
        };
        BinaryInfo {
            service: service.display_name().into(),
            status,
            path: path.display().to_string(),
            size_bytes: exists
                .then(|| std::fs::metadata(&path).ok().map(|metadata| metadata.len()))
                .flatten(),
        }
    }

    fn run_version_cmd(&self, path: &PathBuf) -> Option<String> {
        let output = std::process::Command::new(path)
            .arg("--version")
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let line = String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()?
            .trim()
            .to_string();
        Some(
            line.rsplit_once(' ')
                .map(|(_, version)| version)
                .unwrap_or(&line)
                .trim_start_matches('v')
                .to_string(),
        )
    }

    pub async fn download(
        &self,
        service: ServiceId,
        progress: impl Fn(f32) + Send + 'static,
    ) -> Result<(), String> {
        let release = self
            .releases
            .get(&service)
            .ok_or_else(|| format!("No release for {}", service.display_name()))?;
        let path = Self::binary_path(service);
        let expected = self
            .checksum_for_url(&release.url)
            .unwrap_or_else(|| release.sha256.clone());
        Self::download_file(
            &release.url,
            &path,
            &expected,
            release.size_bytes,
            true,
            progress,
        )
        .await
    }

    pub async fn download_all(
        &mut self,
        progress: impl Fn(ServiceId, f32) + Send + Clone + 'static,
    ) -> Result<(), String> {
        self.fetch_checksums().await?;
        for service in ServiceId::all() {
            if !Self::is_installed(service) {
                let callback = progress.clone();
                self.download(service, move |value| callback(service, value))
                    .await?;
            }
        }
        Ok(())
    }

    pub async fn download_runtime_assets() -> Result<(), String> {
        let mut manager = Self::new();
        manager.fetch_checksums().await?;
        for (key, filename) in [
            ("alkanes_wasm", "alkanes.wasm"),
            ("esplorashrew_wasm", "esplorashrew.wasm"),
        ] {
            let path = get_bin_dir().join(filename);
            if path.exists() {
                continue;
            }
            let component = manager
                .manifest
                .hosted
                .get(key)
                .ok_or_else(|| format!("runtime manifest is missing {key}"))?;
            let asset = &component.asset_pattern;
            let url = format!("{}/{}", manager.manifest.release_base(), asset);
            let expected = manager
                .checksums_cache
                .as_ref()
                .and_then(|checksums| checksums.get(asset))
                .cloned()
                .or_else(|| component.sha256.get("all").cloned())
                .ok_or_else(|| format!("runtime manifest is missing checksum for {key}"))?;
            Self::download_file(&url, &path, &expected, component.size_bytes, false, |_| {})
                .await?;
        }
        Ok(())
    }

    fn checksum_for_url(&self, url: &str) -> Option<String> {
        let filename = url.rsplit('/').next()?;
        self.checksums_cache
            .as_ref()
            .and_then(|checksums| checksums.get(filename))
            .cloned()
    }

    async fn download_file(
        url: &str,
        path: &PathBuf,
        expected: &str,
        _size_bytes: u64,
        executable: bool,
        progress: impl Fn(f32),
    ) -> Result<(), String> {
        progress(0.0);
        let response = reqwest::get(url)
            .await
            .map_err(|e| format!("Download failed: {e}"))?;
        if !response.status().is_success() {
            return Err(format!(
                "Download failed with status: {}",
                response.status()
            ));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|e| format!("Failed to read download: {e}"))?;
        progress(0.9);
        Self::verify_checksum(&bytes, expected, path.to_string_lossy().as_ref())?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create runtime directory: {e}"))?;
        }
        std::fs::write(path, bytes).map_err(|e| format!("Failed to write runtime asset: {e}"))?;
        #[cfg(unix)]
        if executable {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(path)
                .map_err(|e| format!("Failed to read permissions: {e}"))?
                .permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(path, permissions)
                .map_err(|e| format!("Failed to set permissions: {e}"))?;
        }
        #[cfg(target_os = "macos")]
        if executable {
            let _ = std::process::Command::new("codesign")
                .args(["-s", "-", "-f"])
                .arg(path)
                .status();
        }
        progress(1.0);
        Ok(())
    }

    fn verify_checksum(bytes: &[u8], expected: &str, name: &str) -> Result<(), String> {
        if expected.len() != 64 || !expected.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(format!("Invalid SHA-256 checksum configured for {name}"));
        }
        let actual = hex::encode(Sha256::digest(bytes));
        if actual != expected.to_ascii_lowercase() {
            return Err(format!(
                "Checksum verification failed for {name}: expected {expected}, got {actual}"
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_maps_only_qubitcoind_on_supported_platforms() {
        let manifest = RuntimeManifest::load();
        for (os, arch) in [("darwin", "arm64"), ("linux", "x86_64")] {
            let releases = BinaryManager::get_releases_for(&manifest, os, arch);
            assert_eq!(releases.len(), 1);
            assert!(releases.contains_key(&ServiceId::Qubitcoind));
        }
        assert!(BinaryManager::get_releases_for(&manifest, "linux", "arm64").is_empty());
    }

    #[test]
    fn runtime_manifest_has_exact_sources_and_assets() {
        let manifest = RuntimeManifest::load();
        assert_eq!(
            manifest.sources["qubitcoin"].revision,
            "e7f2f9d8844bdc7662030d98abb0544cc3e5a8da"
        );
        assert_eq!(
            manifest.sources["alkanes-wasm"].revision,
            "5b7f43567b828d0bb7b8907ce78fa0242943c54d"
        );
        assert_eq!(
            manifest.sources["esplorashrew-wasm"].revision,
            "7f7660908cdb54d12540ac6a8b337ef6a70e8057"
        );
        assert_eq!(manifest.hosted.len(), 3);
    }

    #[test]
    fn checksum_verification_rejects_tampering_and_missing_hashes() {
        let digest = hex::encode(Sha256::digest(b"runtime"));
        assert!(BinaryManager::verify_checksum(b"runtime", &digest, "fixture").is_ok());
        assert!(BinaryManager::verify_checksum(b"tampered", &digest, "fixture").is_err());
        assert!(BinaryManager::verify_checksum(b"runtime", "", "fixture").is_err());
    }
}
