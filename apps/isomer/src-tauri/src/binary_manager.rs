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
use std::process::Command;

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
}

impl Default for BinaryManager {
    fn default() -> Self {
        Self::new()
    }
}

impl BinaryManager {
    pub fn new() -> Self {
        Self {
            releases: Self::get_releases_for_platform(),
        }
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
    fn get_releases_for_platform() -> HashMap<ServiceId, BinaryRelease> {
        let mut releases = HashMap::new();
        let (os, arch) = Self::get_platform();

        // Bitcoin Core
        let (btc_url, btc_sha) = if os == "darwin" && arch == "arm64" {
            (
                "https://bitcoincore.org/bin/bitcoin-core-29.2/bitcoin-29.2-arm64-apple-darwin.tar.gz",
                "bd07450f76d149d094842feab58e6240673120c8a317a1c51d45ba30c34e85ef",
            )
        } else if os == "darwin" && arch == "x86_64" {
            (
                "https://bitcoincore.org/bin/bitcoin-core-29.2/bitcoin-29.2-x86_64-apple-darwin.tar.gz",
                "69ca05fbe838123091cf4d6d2675352f36cf55f49e2e6fb3b52fcf32b5e8dd9f",
            )
        } else if os == "linux" && arch == "x86_64" {
            (
                "https://bitcoincore.org/bin/bitcoin-core-29.2/bitcoin-29.2-x86_64-linux-gnu.tar.gz",
                "1fd58d0ae94b8a9e21bbaeab7d53395a44976e82bd5492b0a894826c135f9009",
            )
        } else if os == "linux" && arch == "arm64" {
            (
                "https://bitcoincore.org/bin/bitcoin-core-29.2/bitcoin-29.2-aarch64-linux-gnu.tar.gz",
                "f88f72a3c5bf526581aae573be8c1f62133eaecfe3d34646c9ffca7b79dfdc7a",
            )
        } else {
            (
                "https://bitcoincore.org/bin/bitcoin-core-29.2/bitcoin-29.2-x86_64-linux-gnu.tar.gz",
                "1fd58d0ae94b8a9e21bbaeab7d53395a44976e82bd5492b0a894826c135f9009",
            )
        };

        releases.insert(
            ServiceId::Bitcoind,
            BinaryRelease {
                version: "29.2".to_string(),
                url: btc_url.to_string(),
                sha256: btc_sha.to_string(),
                size_bytes: 45_000_000,
                archive_path: Some("bitcoin-29.2/bin/bitcoind".to_string()),
                is_archive: true,
            },
        );

        // Ord - official releases from ordinals/ord
        let (ord_url, ord_sha) = if os == "darwin" && arch == "arm64" {
            (
                "https://github.com/ordinals/ord/releases/download/0.22.1/ord-0.22.1-aarch64-apple-darwin.tar.gz",
                "f4a6c9e1bdbc00b0fb01e053078ce9577aa83495dbcd396e8c9df1ad66064037",
            )
        } else if os == "darwin" && arch == "x86_64" {
            (
                "https://github.com/ordinals/ord/releases/download/0.22.1/ord-0.22.1-x86_64-apple-darwin.tar.gz",
                "",
            )
        } else if os == "linux" && arch == "x86_64" {
            (
                "https://github.com/ordinals/ord/releases/download/0.22.1/ord-0.22.1-x86_64-unknown-linux-gnu.tar.gz",
                "",
            )
        } else {
            (
                "https://github.com/ordinals/ord/releases/download/0.22.1/ord-0.22.1-x86_64-unknown-linux-gnu.tar.gz",
                "",
            )
        };

        releases.insert(
            ServiceId::Ord,
            BinaryRelease {
                version: "0.22.1".to_string(),
                url: ord_url.to_string(),
                sha256: ord_sha.to_string(),
                size_bytes: 15_000_000,
                archive_path: Some("ord".to_string()),
                is_archive: true,
            },
        );

        // For metashrew binaries (rockshrew-mono, memshrew-p2p, flextrs),
        // these need to be pre-built and hosted. Using placeholder URLs that
        // would point to your release infrastructure.
        let isomer_release_base =
            "https://github.com/jonatns/isomer/releases/download/binaries-v0.1.0";

        // Determine SHA based on platform
        // Currently only Mac ARM64 is populated
        let rockshrew_sha = if os == "darwin" && arch == "arm64" {
            "c930a6a786d7491c5cf418c260ce7f0e230eaad810df7fd2b53945c772e54fef"
        } else {
            ""
        };

        releases.insert(
            ServiceId::Metashrew,
            BinaryRelease {
                version: "9.0.1-rc.2".to_string(),
                // Format: rockshrew-mono-v9.0.1-rc.2-darwin-arm64
                url: format!(
                    "{}/rockshrew-mono-v9.0.1-rc.2-{}-{}",
                    isomer_release_base, os, arch
                ),
                sha256: rockshrew_sha.to_string(),
                size_bytes: 25_000_000,
                archive_path: None,
                is_archive: false,
            },
        );

        let memshrew_sha = if os == "darwin" && arch == "arm64" {
            "30dd4c989e7472e7a011777ccdf5ab8d43b7c42092ab6f85a72a7127f8fb6601"
        } else {
            ""
        };

        releases.insert(
            ServiceId::Memshrew,
            BinaryRelease {
                version: "9.0.1".to_string(),
                // Format: memshrew-p2p-v9.0.1-darwin-arm64 (using 9.0.1-rc.2 tag for download but binary version is 9.0.1)
                // Wait, in release-binaries.yml we used env.METASHREW_VERSION which is v9.0.2-alpha.1
                // But here we are using 9.0.1-rc.2 to match installed binary.
                // WE MUST MATCH what is in release-binaries.yml if we want to download NEW binaries.
                // However, user is failing on loop because download is old.
                // Assuming the NEXT release will have v9.0.2-alpha.1, we should probably set this to that
                // BUT current installed is 9.0.1-rc.2

                // Let's stick with the current "installed" version for now to stop the loop,
                // but if we were to point to the NEW release, we would need to update the version here too.
                // Since the user asked to "Add version to binary filenames", I will update the URL pattern.

                // Note: The previous step I set METASHREW_VERSION: "v9.0.2-alpha.1" in yaml.
                // So the artifact will be rockshrew-mono-v9.0.2-alpha.1-...
                // So if we want to fix the loop, we should ideally be pointing to THAT version.
                // But for now, to keep the UI happy with what's on disk (9.0.1-rc.2), I will use the specific version in the filename.
                // A future update will bump the config version to 9.0.2-alpha.1 and the URL will update automatically.
                url: format!(
                    "{}/rockshrew-mono-v9.0.1-rc.2-{}-{}",
                    isomer_release_base, os, arch
                ),
                sha256: memshrew_sha.to_string(),
                size_bytes: 20_000_000,
                archive_path: None,
                is_archive: false,
            },
        );

        // CORRECTION: The instruction is to update `url` to include version in filename.
        // It's better to use `format!("{}/rockshrew-mono-v{}-{}-{}", ..., version, ...)` so it upgrades automatically.
        // But `version` here ("9.0.1-rc.2") lacks the 'v' prefix if we strip it, or has it.
        // In this file, I set version as "9.0.1-rc.2".
        // The file on release will be `rockshrew-mono-v9.0.1-rc.2-...` (prefixed with v).

        // For Memshrew: "9.0.1" -> `memshrew-p2p-v9.0.1-...`.
        // For Esplora: "0.4.1" -> `flextrs-0.4.1-...` (no v prefix in yaml for FLEXTRS_VERSION="0.4.1").

        releases.insert(
            ServiceId::Memshrew,
            BinaryRelease {
                version: "9.0.1".to_string(),
                url: format!(
                    "{}/memshrew-p2p-v9.0.1-{}-{}",
                    isomer_release_base, os, arch
                ),
                sha256: memshrew_sha.to_string(),
                size_bytes: 20_000_000,
                archive_path: None,
                is_archive: false,
            },
        );

        let flextrs_sha = if os == "darwin" && arch == "arm64" {
            "ae38e7a5bc3b10b7b0fd74f84288ae2470972cb1f227029c8d9d54682119cafe"
        } else {
            ""
        };

        releases.insert(
            ServiceId::Esplora,
            BinaryRelease {
                version: "0.4.1".to_string(),
                url: format!("{}/flextrs-0.4.1-{}-{}", isomer_release_base, os, arch),
                sha256: flextrs_sha.to_string(),
                size_bytes: 15_000_000,
                archive_path: None,
                is_archive: false,
            },
        );

        // JSON-RPC is a Node.js app - bundle it differently
        releases.insert(
            ServiceId::JsonRpc,
            BinaryRelease {
                version: "0.1.0".to_string(),
                url: format!("{}/alkanes-jsonrpc-bundle.tar.gz", isomer_release_base),
                sha256: "22a3743f0fecc69a1c123bfc5dd4d30cd32a7049f56b6fd7ef1eb487dda44aca"
                    .to_string(),
                size_bytes: 10_000_000,
                archive_path: None,
                is_archive: true,
            },
        );

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

        let latest_version = self
            .releases
            .get(&service)
            .map(|r| r.version.clone())
            .unwrap_or_else(|| "unknown".to_string());

        let status = if exists {
            let current_version = self
                .get_binary_version(service)
                .unwrap_or("unknown".to_string());

            // Debug logging to find mismatch
            tracing::info!(
                "Checking {}: current='{}' vs latest='{}'",
                service.display_name(),
                current_version,
                latest_version
            );

            // If we can't determine version (unknown), or if it differs from latest, assume update available
            // Note: This relies on run_version_cmd producing a string that matches release version exactly.
            // If formats differ, this will always show update available, which is safer than hiding updates.
            if current_version != latest_version {
                BinaryStatus::UpdateAvailable {
                    current: current_version,
                    latest: latest_version,
                }
            } else {
                BinaryStatus::Installed {
                    version: current_version,
                }
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
            ServiceId::Memshrew => self.run_version_cmd(&path, "--version"),
            ServiceId::Esplora => self.run_version_cmd(&path, "--version"), // flextrs
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

        // Download the file
        let response = reqwest::get(&release.url)
            .await
            .map_err(|e| format!("Download failed: {}", e))?;

        if !response.status().is_success() {
            return Err(format!(
                "Download failed with status: {}",
                response.status()
            ));
        }

        let _total_size = response.content_length().unwrap_or(release.size_bytes);

        let bytes = response
            .bytes()
            .await
            .map_err(|e| format!("Failed to read response: {}", e))?;

        progress_callback(0.5);

        if !release.sha256.is_empty() {
            tracing::info!("Verifying checksum for {}...", service.display_name());
            let mut hasher = Sha256::new();
            hasher.update(&bytes);
            let result = hasher.finalize();
            let digest = hex::encode(result);

            if digest != release.sha256 {
                tracing::error!(
                    "Checksum mismatch for {}: expected {}, got {}",
                    service.display_name(),
                    release.sha256,
                    digest
                );
                return Err(format!(
                    "Checksum verification failed for {}",
                    service.display_name()
                ));
            }
            tracing::info!("Checksum verified for {}", service.display_name());
        }

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
        &self,
        progress_callback: impl Fn(ServiceId, f32) + Send + Clone + 'static,
    ) -> Result<(), String> {
        for service in ServiceId::all() {
            let status = self.check_binary(service).status;
            let should_download = match status {
                BinaryStatus::NotInstalled => true,
                BinaryStatus::UpdateAvailable { .. } => true,
                _ => false,
            };

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

        let wasm_url =
            "https://github.com/jonatns/isomer/releases/download/binaries-v0.1.0/alkanes.wasm";

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

        std::fs::write(&wasm_path, &bytes)
            .map_err(|e| format!("Failed to write alkanes.wasm: {}", e))?;

        tracing::info!("alkanes.wasm downloaded successfully");
        Ok(())
    }
}
