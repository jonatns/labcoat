//! Configuration management for Isomer
//!
//! Handles user preferences and service configuration

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Service ports configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortConfig {
    /// Labcoat Network's JSON-RPC port.
    pub rpc: u16,
}

impl Default for PortConfig {
    fn default() -> Self {
        Self { rpc: 18443 }
    }
}

/// The configuration version this build reads. Version 3 has a single RPC
/// port; older files configured Qubitcoin and are ignored.
const CONFIG_SCHEMA: u32 = 3;

/// Complete Isomer configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IsomerConfig {
    pub schema: u32,
    pub ports: PortConfig,
}

impl Default for IsomerConfig {
    fn default() -> Self {
        Self {
            schema: CONFIG_SCHEMA,
            ports: PortConfig::default(),
        }
    }
}

impl IsomerConfig {
    /// Get the config file path
    pub fn config_path() -> PathBuf {
        get_data_dir().join("config.json")
    }

    /// Load config from disk, or the default when there is none.
    /// `LABCOAT_RPC_PORT` overrides the port, so a second data home can run
    /// its own network beside the first.
    pub fn load() -> Self {
        let mut config = Self::load_file();
        if let Some(port) = port_override(std::env::var("LABCOAT_RPC_PORT").ok().as_deref()) {
            config.ports.rpc = port;
        }
        config
    }

    fn load_file() -> Self {
        let path = Self::config_path();
        if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(content) => match serde_json::from_str::<Self>(&content) {
                    Ok(config) if config.schema == CONFIG_SCHEMA => return config,
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

/// The port `LABCOAT_RPC_PORT` names, when it names a usable one.
fn port_override(value: Option<&str>) -> Option<u16> {
    let value = value?.trim();
    if value.is_empty() {
        return None;
    }
    match value.parse::<u16>() {
        Ok(port) if port != 0 => Some(port),
        _ => {
            tracing::warn!("Ignoring LABCOAT_RPC_PORT={value}: not a port number");
            None
        }
    }
}

/// The directory the platform keeps application data in, or
/// `LABCOAT_DATA_HOME`.
fn data_home() -> PathBuf {
    std::env::var_os("LABCOAT_DATA_HOME")
        .map(PathBuf::from)
        .or_else(dirs::data_dir)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Labcoat's one data directory: `Labcoat` on macOS, `labcoat` on Linux.
/// The chain, its snapshots and logs, the network configuration, the
/// keystores, and the versioned runtimes all live below it.
pub fn get_data_dir() -> PathBuf {
    labcoat_product_dir(&data_home(), std::env::consts::OS)
}

/// Where releases up to 0.3.0 kept the chain, under the project's earlier
/// name. Nothing is read from or written to it; it is only how a
/// `qubitcoind` one of those releases left running is recognized.
pub fn get_legacy_data_dir() -> PathBuf {
    data_home().join("Isomer")
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
/// `LABCOAT_RUNTIME_DIR` names it directly, so a network with its own data
/// home (an e2e run's) can share the runtime already installed.
pub fn get_bin_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("LABCOAT_RUNTIME_DIR").filter(|dir| !dir.is_empty()) {
        return PathBuf::from(dir);
    }
    labcoat_runtime_dir(
        &data_home(),
        std::env::consts::OS,
        &format!("cli-v{}", env!("CARGO_PKG_VERSION")),
    )
}

/// Get the directory Labcoat keeps keystores in, outside every project.
/// `reset` never touches it.
pub fn get_wallets_dir() -> PathBuf {
    get_data_dir().join("wallets")
}

/// The directory a snapshot copies and a restore replaces: the network's.
pub fn get_runtime_dir() -> PathBuf {
    get_network_dir()
}

/// Get the logs directory
pub fn get_logs_dir() -> PathBuf {
    get_data_dir().join("logs")
}

/// Where Labcoat Network saves its chain, faucet, and instance id.
pub fn get_network_dir() -> PathBuf {
    get_data_dir().join("network")
}

/// Where compiled indexer modules are cached, per release.
pub fn get_compiled_dir() -> PathBuf {
    get_bin_dir().join("compiled")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_real_port_number_overrides_the_configured_port() {
        assert_eq!(port_override(Some("18555")), Some(18555));
        assert_eq!(port_override(Some(" 18555 ")), Some(18555));
        assert_eq!(port_override(None), None);
        assert_eq!(port_override(Some("")), None);
        assert_eq!(port_override(Some("0")), None);
        assert_eq!(port_override(Some("70000")), None);
        assert_eq!(port_override(Some("rpc")), None);
    }

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
    fn v3_config_has_one_rpc_port() {
        let value = serde_json::to_value(IsomerConfig::default()).unwrap();
        assert_eq!(value["schema"], 3);
        assert_eq!(value["ports"], serde_json::json!({"rpc": 18443}));
    }
}
