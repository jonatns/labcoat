//! Shared devnet state types
//!
//! Pure serde types describing services, accounts, and system status.

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

/// Overall system status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemStatus {
    pub services: Vec<ServiceInfo>,
    pub block_height: u64,
    pub mempool_size: usize,
    pub is_ready: bool,
}
