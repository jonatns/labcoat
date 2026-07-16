//! Shared devnet state types
//!
//! Pure serde types describing services, accounts, and system status.
//! (The Tauri `AppState` wrapper lives in the desktop app.)

use serde::{Deserialize, Serialize};

/// Status of a managed service
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ServiceStatus {
    Stopped,
    Starting,
    Running,
    Error(String),
}

/// Information about a single service
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceInfo {
    pub id: String,
    pub name: String,
    pub status: String, // "stopped", "running", "error" // Simplified for frontend
    pub pid: Option<u32>,
    pub port: u16,
    pub uptime_secs: Option<u64>,
    pub version: Option<String>,
}

/// Pre-funded development account
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub index: usize,
    pub address: String,
    pub private_key: String,
    pub balance_sats: u64,
}

/// Detailed address information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddressInfo {
    pub address: String,
    pub type_label: String,
    pub index: usize,
}

/// Alkanes-CLI wallet
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlkanesWallet {
    pub name: String,
    pub file_path: String,
    pub balance: Option<String>,
    pub addresses: Vec<AddressInfo>,
}

/// Overall system status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemStatus {
    pub services: Vec<ServiceInfo>,
    pub block_height: u64,
    pub mempool_size: usize,
    pub is_ready: bool,
}
