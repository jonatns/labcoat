//! Configuration management for Isomer
//!
//! Handles user preferences and service configuration

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Service ports configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortConfig {
    pub bitcoind_rpc: u16,
    pub bitcoind_p2p: u16,
    pub metashrew: u16,
    pub memshrew: u16,
    pub ord: u16,
    pub esplora_http: u16,
    pub esplora_electrum: u16,
    pub jsonrpc: u16,
}

impl Default for PortConfig {
    fn default() -> Self {
        Self {
            bitcoind_rpc: 18443,
            bitcoind_p2p: 18444,
            metashrew: 8080,
            memshrew: 8081,
            ord: 8090,
            esplora_http: 50010,
            esplora_electrum: 50001,
            jsonrpc: 18888,
        }
    }
}

/// Bitcoin Core configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BitcoindConfig {
    pub rpc_user: String,
    pub rpc_password: String,
    pub fallback_fee: f64,
}

impl Default for BitcoindConfig {
    fn default() -> Self {
        Self {
            rpc_user: "isomer".to_string(),
            rpc_password: "isomer".to_string(),
            fallback_fee: 0.00001,
        }
    }
}

/// Mining configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiningConfig {
    /// Enable auto-mining on new mempool transactions
    pub auto_mine: bool,
    /// Block interval in milliseconds when auto-mining
    pub block_interval_ms: u64,
    /// Number of blocks to mine on startup to fund accounts
    pub initial_blocks: u32,
}

impl Default for MiningConfig {
    fn default() -> Self {
        Self {
            auto_mine: true,
            block_interval_ms: 1000,
            initial_blocks: 101, // Makes coinbase spendable
        }
    }
}

/// Complete Isomer configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IsomerConfig {
    pub ports: PortConfig,
    pub bitcoind: BitcoindConfig,
    pub mining: MiningConfig,
    /// Mnemonic for deterministic wallet generation (optional)
    pub mnemonic: Option<String>,
}

impl Default for IsomerConfig {
    fn default() -> Self {
        Self {
            ports: PortConfig::default(),
            bitcoind: BitcoindConfig::default(),
            mining: MiningConfig::default(),
            mnemonic: None,
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
                Ok(content) => match serde_json::from_str(&content) {
                    Ok(config) => return config,
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
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Isomer")
}

/// Get the binary directory
pub fn get_bin_dir() -> PathBuf {
    get_data_dir().join("bin")
}

/// Get the runtime data directory (bitcoin data, indexes, etc)
pub fn get_runtime_dir() -> PathBuf {
    get_data_dir().join("data")
}

/// Get the logs directory
pub fn get_logs_dir() -> PathBuf {
    get_data_dir().join("logs")
}
