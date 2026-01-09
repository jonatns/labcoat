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

        // Bitcoin Core - official releases
        let btc_url = if os == "darwin" && arch == "arm64" {
            "https://bitcoincore.org/bin/bitcoin-core-28.0/bitcoin-28.0-arm64-apple-darwin.tar.gz"
        } else if os == "darwin" && arch == "x86_64" {
            "https://bitcoincore.org/bin/bitcoin-core-28.0/bitcoin-28.0-x86_64-apple-darwin.tar.gz"
        } else if os == "linux" && arch == "x86_64" {
            "https://bitcoincore.org/bin/bitcoin-core-28.0/bitcoin-28.0-x86_64-linux-gnu.tar.gz"
        } else if os == "linux" && arch == "arm64" {
            "https://bitcoincore.org/bin/bitcoin-core-28.0/bitcoin-28.0-aarch64-linux-gnu.tar.gz"
        } else {
            "https://bitcoincore.org/bin/bitcoin-core-28.0/bitcoin-28.0-x86_64-linux-gnu.tar.gz"
        };

        releases.insert(
            ServiceId::Bitcoind,
            BinaryRelease {
                version: "28.0".to_string(),
                url: btc_url.to_string(),
                sha256: "".to_string(), // Would verify in production
                size_bytes: 45_000_000,
                archive_path: Some("bitcoin-28.0/bin/bitcoind".to_string()),
                is_archive: true,
            },
        );

        // Ord - official releases from ordinals/ord
        let ord_url = if os == "darwin" && arch == "arm64" {
            "https://github.com/ordinals/ord/releases/download/0.22.1/ord-0.22.1-aarch64-apple-darwin.tar.gz"
        } else if os == "darwin" && arch == "x86_64" {
            "https://github.com/ordinals/ord/releases/download/0.22.1/ord-0.22.1-x86_64-apple-darwin.tar.gz"
        } else if os == "linux" && arch == "x86_64" {
            "https://github.com/ordinals/ord/releases/download/0.22.1/ord-0.22.1-x86_64-unknown-linux-gnu.tar.gz"
        } else {
            "https://github.com/ordinals/ord/releases/download/0.22.1/ord-0.22.1-x86_64-unknown-linux-gnu.tar.gz"
        };

        releases.insert(
            ServiceId::Ord,
            BinaryRelease {
                version: "0.22.1".to_string(),
                url: ord_url.to_string(),
                sha256: "".to_string(),
                size_bytes: 15_000_000,
                archive_path: Some("ord".to_string()),
                is_archive: true,
            },
        );

        // For metashrew binaries (rockshrew-mono, memshrew-p2p, flextrs),
        // these need to be pre-built and hosted. Using placeholder URLs that 
        // would point to your release infrastructure.
        let isomer_release_base = "https://github.com/jonatns/isomer/releases/download/binaries-v0.1.0";

        releases.insert(
            ServiceId::Metashrew,
            BinaryRelease {
                version: "8.8.4".to_string(),
                url: format!("{}/rockshrew-mono-{}-{}", isomer_release_base, os, arch),
                sha256: "".to_string(),
                size_bytes: 25_000_000,
                archive_path: None,
                is_archive: false,
            },
        );

        releases.insert(
            ServiceId::Memshrew,
            BinaryRelease {
                version: "8.8.4".to_string(),
                url: format!("{}/memshrew-p2p-{}-{}", isomer_release_base, os, arch),
                sha256: "".to_string(),
                size_bytes: 20_000_000,
                archive_path: None,
                is_archive: false,
            },
        );

        releases.insert(
            ServiceId::Esplora,
            BinaryRelease {
                version: "0.1.0".to_string(),
                url: format!("{}/flextrs-{}-{}", isomer_release_base, os, arch),
                sha256: "".to_string(),
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
                sha256: "".to_string(),
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

        let status = if exists {
            BinaryStatus::Installed {
                version: self
                    .releases
                    .get(&service)
                    .map(|r| r.version.clone())
                    .unwrap_or_else(|| "unknown".to_string()),
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

        Err(format!(
            "Binary '{}' not found in archive",
            archive_path
        ))
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

        let total_size = response.content_length().unwrap_or(release.size_bytes);

        let bytes = response
            .bytes()
            .await
            .map_err(|e| format!("Failed to read response: {}", e))?;

        progress_callback(0.5);

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

        progress_callback(1.0);
        tracing::info!("{} downloaded successfully", service.display_name());
        Ok(())
    }

    /// Download all missing binaries
    pub async fn download_all(
        &self,
        progress_callback: impl Fn(ServiceId, f32) + Send + Clone + 'static,
    ) -> Result<(), String> {
        for service in ServiceId::all() {
            if !Self::is_installed(service) {
                let cb = progress_callback.clone();
                self.download(service, move |p| cb(service, p)).await?;
            }
        }
        Ok(())
    }

    /// Verify a binary's checksum
    pub fn verify_checksum(service: ServiceId, expected: &str) -> Result<bool, String> {
        let path = Self::get_binary_path(service);
        let data = std::fs::read(&path).map_err(|e| format!("Failed to read binary: {}", e))?;

        let mut hasher = Sha256::new();
        hasher.update(&data);
        let result = hasher.finalize();
        let actual = hex::encode(result);

        Ok(actual == expected)
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

        let wasm_url = "https://github.com/jonatns/isomer/releases/download/binaries-v0.1.0/alkanes.wasm";
        
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
