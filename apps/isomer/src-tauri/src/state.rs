//! Application state management
//!
//! Tracks service status, accounts, and runtime data

use crate::config::IsomerConfig;
use crate::process_manager::ProcessManager;
use serde::{Deserialize, Serialize};
use tauri::Emitter;

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

/// Main application state
pub struct AppState {
    pub config: IsomerConfig,
    pub process_manager: ProcessManager,
    pub accounts: Vec<Account>,
    pub block_height: u64,
    pub mempool_size: usize,
    app_handle: tauri::AppHandle,
}

impl AppState {
    pub fn new(app_handle: tauri::AppHandle) -> Self {
        Self {
            config: IsomerConfig::load(),
            process_manager: ProcessManager::new(),
            accounts: Vec::new(),
            block_height: 0,
            mempool_size: 0,
            app_handle,
        }
    }

    /// Get the current system status
    pub fn get_status(&mut self) -> SystemStatus {
        let services = self.process_manager.get_all_status();
        let is_ready = services.iter().all(|s| s.status == "running");

        SystemStatus {
            services,
            block_height: self.block_height,
            mempool_size: self.mempool_size,
            is_ready,
        }
    }

    /// Emit an event to the frontend
    pub fn emit<S: Serialize + Clone>(&self, event: &str, payload: S) {
        if let Err(e) = self.app_handle.emit(event, payload) {
            tracing::error!("Failed to emit event {}: {}", event, e);
        }
    }
}
