//! Configuration management for Isomer
//!
//! Handles user preferences and service configuration

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Service ports configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortConfig {
    pub qubitcoin_rpc: u16,
    pub qubitcoin_p2p: u16,
}

impl Default for PortConfig {
    fn default() -> Self {
        Self {
            qubitcoin_rpc: 18443,
            qubitcoin_p2p: 18444,
        }
    }
}

/// Complete Isomer configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IsomerConfig {
    pub schema: u32,
    pub ports: PortConfig,
}

impl Default for IsomerConfig {
    fn default() -> Self {
        Self {
            schema: 2,
            ports: PortConfig::default(),
        }
    }
}

impl IsomerConfig {
    /// Get the config file path
    pub fn config_path() -> PathBuf {
        get_data_dir().join("config.json")
    }

    /// Load config from disk, or create default if not exists
    pub fn load() -> Self {
        let path = Self::config_path();
        if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(content) => match serde_json::from_str::<Self>(&content) {
                    Ok(config) if config.schema == 2 => return config,
                    Ok(_) => tracing::warn!("Ignoring legacy Isomer configuration"),
                    Err(e) => tracing::warn!("Failed to parse config: {}", e),
                },
                Err(e) => tracing::warn!("Failed to read config: {}", e),
            }
        }
        Self::default()
    }

    /// Save config to disk
    pub fn save(&self) -> Result<(), std::io::Error> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(path, content)
    }
}

/// Get the Isomer data directory
pub fn get_data_dir() -> PathBuf {
    std::env::var_os("LABCOAT_DATA_HOME")
        .map(PathBuf::from)
        .or_else(dirs::data_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Isomer")
}

fn labcoat_product_dir(data_dir: &Path, target_os: &str) -> PathBuf {
    let product_dir = if target_os == "linux" {
        "labcoat"
    } else {
        "Labcoat"
    };
    data_dir.join(product_dir)
}

fn labcoat_runtime_dir(data_dir: &Path, target_os: &str, release_tag: &str) -> PathBuf {
    labcoat_product_dir(data_dir, target_os)
        .join("runtimes")
        .join(release_tag)
}

/// Get the Labcoat-managed runtime directory for this exact product version.
pub fn get_bin_dir() -> PathBuf {
    let data_dir = std::env::var_os("LABCOAT_DATA_HOME")
        .map(PathBuf::from)
        .or_else(dirs::data_dir)
        .unwrap_or_else(|| PathBuf::from("."));
    labcoat_runtime_dir(
        &data_dir,
        std::env::consts::OS,
        &format!("cli-v{}", env!("CARGO_PKG_VERSION")),
    )
}

/// Get the runtime data directory (bitcoin data, indexes, etc)
pub fn get_runtime_dir() -> PathBuf {
    get_data_dir().join("runtime-v2")
}

/// Get the logs directory
pub fn get_logs_dir() -> PathBuf {
    get_data_dir().join("logs")
}

/// Qubitcoin's network-specific data root.
pub fn get_qubitcoin_dir() -> PathBuf {
    get_runtime_dir().join("qubitcoin")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macos_runtime_is_versioned_below_labcoat_application_support() {
        let base = Path::new("/Users/test/Library/Application Support");
        assert_eq!(
            labcoat_runtime_dir(base, "macos", "cli-v1.2.3"),
            PathBuf::from("/Users/test/Library/Application Support/Labcoat/runtimes/cli-v1.2.3")
        );
    }

    #[test]
    fn linux_runtime_is_versioned_below_xdg_data_home() {
        let xdg_data_home = Path::new("/home/test/custom-data");
        assert_eq!(
            labcoat_runtime_dir(xdg_data_home, "linux", "cli-v1.2.3"),
            PathBuf::from("/home/test/custom-data/labcoat/runtimes/cli-v1.2.3")
        );
    }

    #[test]
    fn v2_config_has_only_qubitcoin_ports() {
        let value = serde_json::to_value(IsomerConfig::default()).unwrap();
        assert_eq!(value["schema"], 2);
        assert_eq!(
            value["ports"],
            serde_json::json!({"qubitcoin_rpc": 18443, "qubitcoin_p2p": 18444})
        );
    }
}
