//! Binary download and management
//!
//! Handles downloading, verifying, and updating service binaries

use crate::config::get_bin_dir;
use crate::process_manager::ServiceId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
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
}

/// Manages binary downloads and updates
pub struct BinaryManager {
    releases: HashMap<ServiceId, BinaryRelease>,
}

impl BinaryManager {
    pub fn new() -> Self {
        Self {
            releases: Self::get_latest_releases(),
        }
    }

    /// Get the latest release info for all binaries
    /// TODO: Fetch this from a central manifest URL in production
    fn get_latest_releases() -> HashMap<ServiceId, BinaryRelease> {
        let mut releases = HashMap::new();

        // These would be fetched from GitHub releases or a manifest in production
        // For now, use placeholder values
        releases.insert(
            ServiceId::Bitcoind,
            BinaryRelease {
                version: "28.0".to_string(),
                url: "https://github.com/bitcoin/bitcoin/releases/download/v28.0/bitcoin-28.0-x86_64-apple-darwin.tar.gz".to_string(),
                sha256: "placeholder".to_string(),
                size_bytes: 45_000_000,
            },
        );

        releases.insert(
            ServiceId::Ord,
            BinaryRelease {
                version: "0.21.0".to_string(),
                url: "https://github.com/ordinals/ord/releases/download/0.21.0/ord-0.21.0-x86_64-apple-darwin.tar.gz".to_string(),
                sha256: "placeholder".to_string(),
                size_bytes: 15_000_000,
            },
        );

        // Custom binaries from alkanes ecosystem
        releases.insert(
            ServiceId::Metashrew,
            BinaryRelease {
                version: "0.1.0".to_string(),
                url: "https://github.com/sandshrewmetaprotocols/metashrew/releases/latest".to_string(),
                sha256: "placeholder".to_string(),
                size_bytes: 20_000_000,
            },
        );

        releases.insert(
            ServiceId::Memshrew,
            BinaryRelease {
                version: "0.1.0".to_string(),
                url: "https://github.com/sandshrewmetaprotocols/metashrew/releases/latest".to_string(),
                sha256: "placeholder".to_string(),
                size_bytes: 15_000_000,
            },
        );

        releases.insert(
            ServiceId::Esplora,
            BinaryRelease {
                version: "0.1.0".to_string(),
                url: "https://github.com/kungfuflex/flextrs/releases/latest".to_string(),
                sha256: "placeholder".to_string(),
                size_bytes: 10_000_000,
            },
        );

        releases.insert(
            ServiceId::JsonRpc,
            BinaryRelease {
                version: "0.1.0".to_string(),
                url: "https://github.com/kungfuflex/alkanes-rs/releases/latest".to_string(),
                sha256: "placeholder".to_string(),
                size_bytes: 25_000_000,
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
            // TODO: Check version by running binary --version
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

        // Download the file
        let response = reqwest::get(&release.url)
            .await
            .map_err(|e| format!("Download failed: {}", e))?;

        let total_size = response.content_length().unwrap_or(release.size_bytes);
        let mut downloaded: u64 = 0;

        let bytes = response
            .bytes()
            .await
            .map_err(|e| format!("Failed to read response: {}", e))?;

        downloaded = bytes.len() as u64;
        progress_callback(downloaded as f32 / total_size as f32);

        // For tar.gz files, we'd extract here
        // For now, just write directly (simplified)
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
}
