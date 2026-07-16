//! Binary download and management
//!
//! Handles downloading, verifying, and updating service binaries

use crate::config::get_bin_dir;
use crate::process_manager::ServiceId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;

/// Status of a binary
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
            BinaryStatus::Installed { version } => Some(version),
            BinaryStatus::UpdateAvailable { current, .. } => Some(current),
            _ => None,
        }
    }
}

/// Information about a binary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryInfo {
    pub service: String,
    pub status: BinaryStatus,
    pub path: String,
    pub size_bytes: Option<u64>,
}

/// Binary release information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryRelease {
    pub version: String,
    pub url: String,
    pub sha256: String,
    pub size_bytes: u64,
    /// Path within the archive to the binary (for tar.gz extraction)
    pub archive_path: Option<String>,
    /// Whether this is a tar.gz archive that needs extraction
    pub is_archive: bool,
}

/// Manages binary downloads and updates
pub struct BinaryManager {
    releases: HashMap<ServiceId, BinaryRelease>,
    /// Cached checksums fetched from the release
    checksums_cache: Option<HashMap<String, String>>,
    manifest: RuntimeManifest,
}

const RUNTIME_MANIFEST: &str = include_str!("../../../runtime.json");

#[derive(Debug, Clone, Deserialize)]
struct RuntimeManifest {
    schema: u32,
    active_release: ActiveRelease,
    #[allow(dead_code)] // consumed by release automation from the same manifest
    sources: HashMap<String, RuntimeSource>,
    hosted: HashMap<String, HostedComponent>,
    external: HashMap<String, ExternalComponent>,
}

#[derive(Debug, Clone, Deserialize)]
struct ActiveRelease {
    owner: String,
    repository: String,
    tag: String,
    checksums_asset: Option<String>,
}

#[allow(dead_code)] // consumed by release automation from the same manifest
#[derive(Debug, Clone, Deserialize)]
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
    archive_path: Option<String>,
    sha256: HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ExternalComponent {
    version: String,
    size_bytes: u64,
    platforms: HashMap<String, ExternalPlatform>,
}

#[derive(Debug, Clone, Deserialize)]
struct ExternalPlatform {
    url: String,
    sha256: String,
    archive_path: Option<String>,
}

impl RuntimeManifest {
    fn load() -> Self {
        let manifest: Self =
            serde_json::from_str(RUNTIME_MANIFEST).expect("embedded runtime.json must be valid");
        assert_eq!(manifest.schema, 1, "unsupported runtime.json schema");
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

    /// Fetch checksums from the release
    pub async fn fetch_checksums(&mut self) -> Result<(), String> {
        if self.checksums_cache.is_some() {
            return Ok(());
        }

        let Some(asset) = self.manifest.active_release.checksums_asset.as_ref() else {
            return Ok(());
        };
        let url = format!("{}/{}", self.manifest.release_base(), asset);
        tracing::info!("Fetching checksums from {}", url);

        let client = reqwest::Client::new();
        let response = client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("Failed to fetch checksums: {}", e))?;

        if !response.status().is_success() {
            return Err(format!(
                "Failed to fetch runtime checksums from {}: {}",
                url,
                response.status()
            ));
        }

        let checksums: HashMap<String, String> = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse checksums: {}", e))?;

        tracing::info!("Loaded {} checksums", checksums.len());
        self.checksums_cache = Some(checksums);
        Ok(())
    }

    /// Get checksum for a filename from the cache
    fn get_checksum_for_file(&self, filename: &str) -> Option<String> {
        self.checksums_cache
            .as_ref()
            .and_then(|c| c.get(filename).cloned())
    }

    /// Detect the current platform
    fn get_platform() -> (&'static str, &'static str) {
        let os = if cfg!(target_os = "macos") {
            "darwin"
        } else if cfg!(target_os = "linux") {
            "linux"
        } else if cfg!(target_os = "windows") {
            "windows"
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

    /// Get the latest release info for all binaries based on platform
    fn get_releases_for_platform(manifest: &RuntimeManifest) -> HashMap<ServiceId, BinaryRelease> {
        let (os, arch) = Self::get_platform();
        Self::get_releases_for(manifest, os, arch)
    }

    fn get_releases_for(
        manifest: &RuntimeManifest,
        os: &str,
        arch: &str,
    ) -> HashMap<ServiceId, BinaryRelease> {
        let mut releases = HashMap::new();
        let platform = format!("{}-{}", os, arch);

        for (service, key) in [(ServiceId::Bitcoind, "bitcoind"), (ServiceId::Ord, "ord")] {
            let Some(component) = manifest.external.get(key) else {
                continue;
            };
            let Some(asset) = component.platforms.get(&platform) else {
                continue;
            };
            releases.insert(
                service,
                BinaryRelease {
                    version: component.version.clone(),
                    url: asset.url.clone(),
                    sha256: asset.sha256.clone(),
                    size_bytes: component.size_bytes,
                    archive_path: asset.archive_path.clone(),
                    is_archive: true,
                },
            );
        }

        let release_base = manifest.release_base();
        for (service, key) in [
            (ServiceId::Metashrew, "metashrew"),
            (ServiceId::Esplora, "esplora"),
            (ServiceId::Espo, "espo"),
            (ServiceId::JsonRpc, "jsonrpc"),
        ] {
            let Some(component) = manifest.hosted.get(key) else {
                continue;
            };
            let checksum = component
                .sha256
                .get(&platform)
                .or_else(|| component.sha256.get("all"));
            let Some(checksum) = checksum else {
                continue;
            };
            let asset = component.asset_pattern.replace("{platform}", &platform);
            releases.insert(
                service,
                BinaryRelease {
                    version: component.version.clone(),
                    url: format!("{}/{}", release_base, asset),
                    sha256: checksum.clone(),
                    size_bytes: component.size_bytes,
                    archive_path: component.archive_path.clone(),
                    is_archive: component.asset_pattern.ends_with(".tar.gz"),
                },
            );
        }

        releases
    }

    /// Get the path where a binary should be installed
    fn get_binary_path(service: ServiceId) -> PathBuf {
        get_bin_dir().join(service.binary_name())
    }

    /// Check if a binary is installed
    pub fn is_installed(service: ServiceId) -> bool {
        Self::get_binary_path(service).exists()
    }

    /// Get status of all binaries
    pub fn check_all(&self) -> Vec<BinaryInfo> {
        ServiceId::all()
            .into_iter()
            .map(|service| self.check_binary(service))
            .collect()
    }

    /// Check status of a single binary
    pub fn check_binary(&self, service: ServiceId) -> BinaryInfo {
        let path = Self::get_binary_path(service);
        let exists = path.exists();

        // let latest_version = self
        //     .releases
        //     .get(&service)
        //     .map(|r| r.version.clone())
        //     .unwrap_or_else(|| "unknown".to_string());

        let status = if exists {
            let current_version = self
                .get_binary_version(service)
                .unwrap_or("unknown".to_string());

            // User requested to disable automatic update checks.
            // We always return Installed if the binary exists.
            BinaryStatus::Installed {
                version: current_version,
            }
        } else {
            BinaryStatus::NotInstalled
        };

        let size_bytes = if exists {
            std::fs::metadata(&path).ok().map(|m| m.len())
        } else {
            None
        };

        BinaryInfo {
            service: service.display_name().to_string(),
            status,
            path: path.display().to_string(),
            size_bytes,
        }
    }

    /// Try to get version from installed binary
    fn get_binary_version(&self, service: ServiceId) -> Option<String> {
        let path = Self::get_binary_path(service);
        if !path.exists() {
            return None;
        }

        match service {
            ServiceId::Bitcoind => self.run_version_cmd(&path, "--version"),
            ServiceId::Ord => self.run_version_cmd(&path, "--version"),
            ServiceId::Metashrew => self.run_version_cmd(&path, "--version"),
            ServiceId::Esplora => self.run_version_cmd(&path, "--version"), // flextrs
            ServiceId::Espo => self.run_version_cmd(&path, "--version"),
            ServiceId::JsonRpc => None, // Node script, maybe --version works if executable
        }
    }

    fn run_version_cmd(&self, path: &PathBuf, arg: &str) -> Option<String> {
        use std::process::Command;
        // Run command and capture stdout
        if let Ok(output) = Command::new(path).arg(arg).output() {
            if output.status.success() {
                let s = String::from_utf8_lossy(&output.stdout);
                let line = s.trim().lines().next().unwrap_or("unknown");

                // Clean up version string
                // Format: "Bitcoin Core version v28.0.0" -> "v28.0.0"
                // Format: "memshrew-p2p 9.0.1" -> "9.0.1"
                // Format: "rockshrew-mono 9.0.1-rc.2" -> "9.0.1-rc.2"

                let cleaned = if line.contains("Bitcoin Core") {
                    // Start after "version "
                    if let Some(idx) = line.find("version ") {
                        line[idx + 8..].trim().to_string()
                    } else {
                        line.to_string()
                    }
                } else if let Some(idx) = line.rfind(' ') {
                    // Take the last part after space (usually the version)
                    // Works for "program 1.2.3"
                    line[idx + 1..].trim().to_string()
                } else {
                    line.to_string()
                };

                // Strip leading 'v' if present (e.g. v29.2 -> 29.2)
                let final_version = cleaned.trim_start_matches('v').to_string();

                Some(final_version)
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Extract a tar.gz archive to get a specific binary
    fn extract_binary_from_tar_gz(
        archive_data: &[u8],
        archive_path: &str,
        dest_path: &PathBuf,
    ) -> Result<(), String> {
        use flate2::read::GzDecoder;
        use tar::Archive;

        let gz = GzDecoder::new(archive_data);
        let mut archive = Archive::new(gz);

        for entry in archive
            .entries()
            .map_err(|e| format!("Failed to read archive: {}", e))?
        {
            let mut entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
            let path = entry
                .path()
                .map_err(|e| format!("Failed to get path: {}", e))?;

            if path.ends_with(archive_path) || path.to_string_lossy() == archive_path {
                // Ensure parent directory exists
                if let Some(parent) = dest_path.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| format!("Failed to create directory: {}", e))?;
                }

                // Extract the file
                let mut content = Vec::new();
                entry
                    .read_to_end(&mut content)
                    .map_err(|e| format!("Failed to read binary: {}", e))?;

                std::fs::write(dest_path, &content)
                    .map_err(|e| format!("Failed to write binary: {}", e))?;

                // Make executable on Unix
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mut perms = std::fs::metadata(dest_path)
                        .map_err(|e| format!("Failed to get permissions: {}", e))?
                        .permissions();
                    perms.set_mode(0o755);
                    std::fs::set_permissions(dest_path, perms)
                        .map_err(|e| format!("Failed to set permissions: {}", e))?;
                }

                tracing::info!("Extracted binary to {}", dest_path.display());
                return Ok(());
            }
        }

        Err(format!("Binary '{}' not found in archive", archive_path))
    }

    /// Download a binary
    pub async fn download(
        &self,
        service: ServiceId,
        progress_callback: impl Fn(f32) + Send + 'static,
    ) -> Result<(), String> {
        let release = self
            .releases
            .get(&service)
            .ok_or_else(|| format!("No release info for {}", service.display_name()))?;

        let dest_path = Self::get_binary_path(service);

        // Ensure bin directory exists
        if let Some(parent) = dest_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create bin directory: {}", e))?;
        }

        tracing::info!(
            "Downloading {} from {}",
            service.display_name(),
            release.url
        );

        progress_callback(0.0);

        // Download the file with streaming progress
        let client = reqwest::Client::new();
        let response = client
            .get(&release.url)
            .send()
            .await
            .map_err(|e| format!("Download failed: {}", e))?;

        if !response.status().is_success() {
            return Err(format!(
                "Download failed with status: {}",
                response.status()
            ));
        }

        let total_size = response.content_length().unwrap_or(release.size_bytes);
        let mut downloaded: u64 = 0;
        let mut bytes_vec = Vec::with_capacity(total_size as usize);

        // Stream the response and track progress
        use futures_util::StreamExt;
        let mut stream = response.bytes_stream();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("Failed to read chunk: {}", e))?;
            bytes_vec.extend_from_slice(&chunk);
            downloaded += chunk.len() as u64;

            // Report progress (download is 90% of total work, checksum is last 10%)
            let progress = (downloaded as f32 / total_size as f32) * 0.9;
            progress_callback(progress);
        }

        let bytes = bytes::Bytes::from(bytes_vec);

        progress_callback(0.9);

        // Prefer the promoted release checksum manifest, then the reviewed
        // embedded checksum used by the legacy bootstrap release.
        let filename = release.url.split('/').next_back().unwrap_or("");
        let expected_checksum = self.get_checksum_for_file(filename).or_else(|| {
            if !release.sha256.is_empty() {
                Some(release.sha256.clone())
            } else {
                None
            }
        });

        let expected_checksum = expected_checksum.ok_or_else(|| {
            format!(
                "No checksum available for {}; refusing unverified download",
                service.display_name()
            )
        })?;
        Self::verify_checksum(&bytes, &expected_checksum, service.display_name())?;

        if release.is_archive {
            if let Some(ref archive_path) = release.archive_path {
                // Extract specific binary from archive
                Self::extract_binary_from_tar_gz(&bytes, archive_path, &dest_path)?;
            } else {
                // Extract entire archive to bin directory
                use flate2::read::GzDecoder;
                use tar::Archive;

                let gz = GzDecoder::new(bytes.as_ref());
                let mut archive = Archive::new(gz);
                let bin_dir = get_bin_dir();
                archive
                    .unpack(&bin_dir)
                    .map_err(|e| format!("Failed to extract archive: {}", e))?;
            }
        } else {
            // Direct binary download
            std::fs::write(&dest_path, &bytes)
                .map_err(|e| format!("Failed to write binary: {}", e))?;

            // Make executable on Unix
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(&dest_path)
                    .map_err(|e| format!("Failed to get permissions: {}", e))?
                    .permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(&dest_path, perms)
                    .map_err(|e| format!("Failed to set permissions: {}", e))?;
            }
        }

        // Ad-hoc sign on macOS to prevent SIGKILL
        #[cfg(target_os = "macos")]
        {
            use std::process::Command;
            tracing::info!("Applying ad-hoc signature to {}", dest_path.display());
            let status = Command::new("codesign")
                .arg("-s")
                .arg("-")
                .arg("-f") // Force
                .arg(&dest_path)
                .status()
                .map_err(|e| format!("Failed to run codesign: {}", e))?;

            if !status.success() {
                tracing::warn!("codesign failed for {}", dest_path.display());
                // We don't error out, as it might work anyway or be a dev environment issue
            }
        }

        progress_callback(1.0);
        tracing::info!("{} downloaded successfully", service.display_name());
        Ok(())
    }

    /// Download all missing or outdated binaries
    pub async fn download_all(
        &mut self,
        progress_callback: impl Fn(ServiceId, f32) + Send + Clone + 'static,
    ) -> Result<(), String> {
        // Fetch checksums from release before downloading
        self.fetch_checksums().await?;

        for service in ServiceId::all() {
            let status = self.check_binary(service).status;
            let should_download = matches!(
                status,
                BinaryStatus::NotInstalled | BinaryStatus::UpdateAvailable { .. }
            );

            if should_download {
                let cb = progress_callback.clone();
                self.download(service, move |p| cb(service, p)).await?;
            }
        }
        Ok(())
    }

    /// Download the alkanes.wasm file needed for metashrew
    pub async fn download_alkanes_wasm() -> Result<(), String> {
        let wasm_path = get_bin_dir().join("alkanes.wasm");

        if wasm_path.exists() {
            tracing::info!("alkanes.wasm already exists");
            return Ok(());
        }

        // Ensure bin directory exists
        if let Some(parent) = wasm_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create bin directory: {}", e))?;
        }

        let mut manager = Self::new();
        manager.fetch_checksums().await?;
        let component = manager
            .manifest
            .hosted
            .get("alkanes_wasm")
            .ok_or_else(|| "runtime manifest is missing alkanes_wasm".to_string())?;
        let wasm_url = format!(
            "{}/{}",
            manager.manifest.release_base(),
            component.asset_pattern
        );
        let expected = manager
            .get_checksum_for_file(&component.asset_pattern)
            .or_else(|| component.sha256.get("all").cloned())
            .ok_or_else(|| "runtime manifest is missing the alkanes.wasm checksum".to_string())?;

        tracing::info!("Downloading alkanes.wasm from {}", wasm_url);

        let response = reqwest::get(wasm_url)
            .await
            .map_err(|e| format!("Download failed: {}", e))?;

        if !response.status().is_success() {
            return Err(format!(
                "Download failed with status: {}",
                response.status()
            ));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| format!("Failed to read response: {}", e))?;

        Self::verify_checksum(&bytes, &expected, "alkanes.wasm")?;

        std::fs::write(&wasm_path, &bytes)
            .map_err(|e| format!("Failed to write alkanes.wasm: {}", e))?;

        tracing::info!("alkanes.wasm downloaded successfully");
        Ok(())
    }

    /// Download and extract the Chrome extension
    /// Returns the path to the extension directory
    pub fn is_extension_installed() -> bool {
        use crate::config::get_data_dir;
        let extension_dir = get_data_dir().join("extension");
        extension_dir.join("manifest.json").exists()
    }

    pub async fn download_extension() -> Result<PathBuf, String> {
        Err("the Isomer browser extension is maintained by the manual legacy desktop workflow and is not part of Labcoat runtime releases".to_string())
    }

    fn verify_checksum(bytes: &[u8], expected: &str, name: &str) -> Result<(), String> {
        if expected.len() != 64 || !expected.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(format!("Invalid SHA-256 checksum configured for {}", name));
        }
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let digest = hex::encode(hasher.finalize());
        if digest != expected.to_ascii_lowercase() {
            return Err(format!(
                "Checksum verification failed for {}: expected {}, got {}",
                name, expected, digest
            ));
        }
        tracing::info!("Checksum verified for {}", name);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_manifest_maps_every_declared_platform_without_fallbacks() {
        let manifest = RuntimeManifest::load();
        let darwin_arm = BinaryManager::get_releases_for(&manifest, "darwin", "arm64");
        let linux_x86 = BinaryManager::get_releases_for(&manifest, "linux", "x86_64");
        assert_eq!(darwin_arm.len(), ServiceId::all().len());
        assert_eq!(linux_x86.len(), ServiceId::all().len());

        let darwin_x86 = BinaryManager::get_releases_for(&manifest, "darwin", "x86_64");
        assert!(darwin_x86.contains_key(&ServiceId::Bitcoind));
        assert!(darwin_x86.contains_key(&ServiceId::Ord));
        assert!(darwin_x86.contains_key(&ServiceId::JsonRpc));
        assert!(!darwin_x86.contains_key(&ServiceId::Metashrew));

        let linux_arm = BinaryManager::get_releases_for(&manifest, "linux", "arm64");
        assert!(linux_arm.contains_key(&ServiceId::Bitcoind));
        assert!(linux_arm.contains_key(&ServiceId::JsonRpc));
        assert!(!linux_arm.contains_key(&ServiceId::Ord));
        assert!(!linux_arm.contains_key(&ServiceId::Metashrew));
    }

    #[test]
    fn runtime_manifest_has_reviewed_sources_and_checksums() {
        let manifest = RuntimeManifest::load();
        assert_eq!(manifest.sources.len(), 5);
        for source in manifest.sources.values() {
            assert!(!source.repository.is_empty());
            assert!(!source.revision.is_empty());
            assert!(!matches!(
                source.revision.as_str(),
                "main" | "master" | "develop"
            ));
            assert!(!source.version.is_empty());
        }
        for component in manifest.hosted.values() {
            assert!(!component.sha256.is_empty());
            assert!(component.sha256.values().all(|hash| hash.len() == 64));
        }
    }

    #[test]
    fn checksum_verification_fails_closed() {
        let digest = hex::encode(Sha256::digest(b"runtime"));
        assert!(BinaryManager::verify_checksum(b"runtime", &digest, "fixture").is_ok());
        assert!(BinaryManager::verify_checksum(b"tampered", &digest, "fixture").is_err());
        assert!(BinaryManager::verify_checksum(b"runtime", "", "fixture").is_err());
    }
}
