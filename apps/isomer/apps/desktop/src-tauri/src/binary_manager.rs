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
    /// Cached checksums fetched from the release
    checksums_cache: Option<HashMap<String, String>>,
}

/// URL for the checksums.json file in the release
const CHECKSUMS_URL: &str =
    "https://github.com/jonatns/isomer/releases/download/binaries-v0.1.3/checksums.json";

impl Default for BinaryManager {
    fn default() -> Self {
        Self::new()
    }
}

impl BinaryManager {
    pub fn new() -> Self {
        Self {
            releases: Self::get_releases_for_platform(),
            checksums_cache: None,
        }
    }

    /// Fetch checksums from the release
    pub async fn fetch_checksums(&mut self) -> Result<(), String> {
        if self.checksums_cache.is_some() {
            return Ok(());
        }

        tracing::info!("Fetching checksums from {}", CHECKSUMS_URL);

        let client = reqwest::Client::new();
        let response = client
            .get(CHECKSUMS_URL)
            .send()
            .await
            .map_err(|e| format!("Failed to fetch checksums: {}", e))?;

        if !response.status().is_success() {
            tracing::warn!("Failed to fetch checksums, will skip verification");
            return Ok(());
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
            "https://github.com/jonatns/isomer/releases/download/binaries-v0.1.3";

        // Determine SHA based on platform (placeholder, will be updated on next release)
        let rockshrew_sha = "";

        releases.insert(
            ServiceId::Metashrew,
            BinaryRelease {
                version: "9.0.2-alpha.1".to_string(),
                // Format: rockshrew-mono-darwin-arm64
                url: format!("{}/rockshrew-mono-{}-{}", isomer_release_base, os, arch),
                sha256: rockshrew_sha.to_string(),
                size_bytes: 25_000_000,
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
                url: format!("{}/flextrs-{}-{}", isomer_release_base, os, arch),
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
                sha256: "bedc8928c7c48eb45ab51f9094b06a732ee7542e091cf4e75fd902e8aea84a55"
                    .to_string(),
                size_bytes: 10_000_000,
                archive_path: None,
                is_archive: true,
            },
        );

        // Espo
        let espo_sha = "";

        releases.insert(
            ServiceId::Espo,
            BinaryRelease {
                version: "0.1.0".to_string(),
                url: format!("{}/espo-{}-{}", isomer_release_base, os, arch),
                sha256: espo_sha.to_string(),
                size_bytes: 30_000_000,
                archive_path: None,
                is_archive: false,
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

        // Get checksum - prefer dynamic from checksums.json, fallback to hardcoded
        let filename = release.url.split('/').last().unwrap_or("");
        let expected_checksum = self.get_checksum_for_file(filename).or_else(|| {
            if !release.sha256.is_empty() {
                Some(release.sha256.clone())
            } else {
                None
            }
        });

        if let Some(expected) = expected_checksum {
            tracing::info!("Verifying checksum for {}...", service.display_name());
            let mut hasher = Sha256::new();
            hasher.update(&bytes);
            let result = hasher.finalize();
            let digest = hex::encode(result);

            if digest != expected {
                tracing::error!(
                    "Checksum mismatch for {}: expected {}, got {}",
                    service.display_name(),
                    expected,
                    digest
                );
                return Err(format!(
                    "Checksum verification failed for {}",
                    service.display_name()
                ));
            }
            tracing::info!("Checksum verified for {}", service.display_name());
        } else {
            tracing::warn!(
                "No checksum available for {}, skipping verification",
                service.display_name()
            );
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
        &mut self,
        progress_callback: impl Fn(ServiceId, f32) + Send + Clone + 'static,
    ) -> Result<(), String> {
        // Fetch checksums from release before downloading
        self.fetch_checksums().await?;

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

    /// Download and extract the Chrome extension
    /// Returns the path to the extension directory
    pub fn is_extension_installed() -> bool {
        use crate::config::get_data_dir;
        let extension_dir = get_data_dir().join("extension");
        extension_dir.join("manifest.json").exists()
    }

    pub async fn download_extension() -> Result<PathBuf, String> {
        use crate::config::get_data_dir;

        let extension_dir = get_data_dir().join("extension");
        let manifest_path = extension_dir.join("manifest.json");

        if manifest_path.exists() {
            tracing::info!("Extension already installed at {}", extension_dir.display());
            return Ok(extension_dir);
        }

        // Ensure extension directory exists
        std::fs::create_dir_all(&extension_dir)
            .map_err(|e| format!("Failed to create extension directory: {}", e))?;

        let extension_url =
            "https://github.com/jonatns/isomer/releases/download/binaries-v0.1.3/isomer-extension.zip";

        tracing::info!("Downloading Chrome extension from {}", extension_url);

        let response = reqwest::get(extension_url)
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

        // Extract the zip file
        let reader = std::io::Cursor::new(bytes.as_ref());
        let mut archive = zip::ZipArchive::new(reader)
            .map_err(|e| format!("Failed to read zip archive: {}", e))?;

        for i in 0..archive.len() {
            let mut file = archive
                .by_index(i)
                .map_err(|e| format!("Failed to read zip entry: {}", e))?;

            let outpath = match file.enclosed_name() {
                Some(path) => {
                    // Strip the "dist/" prefix if present
                    let path_str = path.to_string_lossy();
                    if path_str.starts_with("dist/") {
                        extension_dir.join(&path_str[5..])
                    } else {
                        extension_dir.join(path)
                    }
                }
                None => continue,
            };

            if file.name().ends_with('/') {
                // Directory
                std::fs::create_dir_all(&outpath)
                    .map_err(|e| format!("Failed to create directory: {}", e))?;
            } else {
                // File
                if let Some(parent) = outpath.parent() {
                    if !parent.exists() {
                        std::fs::create_dir_all(parent)
                            .map_err(|e| format!("Failed to create parent directory: {}", e))?;
                    }
                }
                let mut outfile = std::fs::File::create(&outpath)
                    .map_err(|e| format!("Failed to create file: {}", e))?;
                std::io::copy(&mut file, &mut outfile)
                    .map_err(|e| format!("Failed to write file: {}", e))?;
            }
        }

        tracing::info!("Extension extracted to {}", extension_dir.display());
        Ok(extension_dir)
    }
}
