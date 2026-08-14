//! The headless Labcoat Network facade
//!
//! One handle over binaries, processes, and chain RPC, used by the
//! `labcoat` CLI and other engine consumers.

use crate::binary_manager::{BinaryInfo, BinaryManager};
use crate::config::{get_logs_dir, get_runtime_dir, IsomerConfig};
use crate::process_manager::{LogEntry, ProcessManager, ServiceId};
use crate::state::{ServiceInfo, SystemStatus};
use std::path::PathBuf;

pub const NETWORK_ID: &str = "labcoat";
pub const BITCOIN_NETWORK_ID: &str = "regtest";

/// A headless Labcoat Network instance.
///
/// A `LabcoatNetwork` is typically short-lived: `labcoat up` starts services that
/// outlive the process, and later invocations observe or control them via
/// ports, log files, and process names. Construction never kills existing
/// processes.
pub struct LabcoatNetwork {
    pub config: IsomerConfig,
    process_manager: ProcessManager,
}

impl LabcoatNetwork {
    /// Create a Labcoat Network handle with config loaded from disk.
    pub fn new() -> Self {
        Self::with_config(IsomerConfig::load())
    }

    pub fn with_config(config: IsomerConfig) -> Self {
        Self {
            config,
            process_manager: ProcessManager::detached(),
        }
    }

    /// Check binary installation status for all services.
    pub fn check_binaries(&self) -> Vec<BinaryInfo> {
        BinaryManager::new().check_all()
    }

    /// Download any missing binaries and indexer WASM modules, reporting
    /// per-service progress through the callback.
    pub async fn ensure_binaries(
        &self,
        progress: impl Fn(ServiceId, f32) + Send + Clone + 'static,
    ) -> Result<(), String> {
        BinaryManager::ensure_bundle(progress).await
    }

    /// Start Qubitcoin and bootstrap the persistent Labcoat faucet.
    pub fn start(&mut self) -> Result<(), String> {
        ProcessManager::kill_orphans();
        let config = self.config.clone();
        self.process_manager.start_all(&config)?;
        crate::faucet::bootstrap_blocking(&config)
    }

    /// Stop all services — both any owned by this handle and detached
    /// ones from earlier `labcoat up` runs (matched by name/port).
    pub fn stop(&mut self) -> Result<(), String> {
        self.process_manager.stop_all()?;
        ProcessManager::kill_orphans();
        Ok(())
    }

    /// Stop services and clear all chain/index data.
    /// (reset_data stops owned processes and force-kills strays itself.)
    pub fn reset(&mut self) -> Result<(), String> {
        self.process_manager.reset_data()
    }

    /// Current system status. Services are probed over their local
    /// endpoints (this handle usually doesn't own the processes), so
    /// `pid`/`uptime` are unavailable here.
    pub async fn status(&mut self) -> SystemStatus {
        let mut services = Vec::new();
        for service in ServiceId::all() {
            let healthy = ProcessManager::probe_health(service, &self.config).await;
            let version = BinaryManager::new()
                .check_binary(service)
                .status
                .into_version();
            services.push(ServiceInfo {
                id: service.id().to_string(),
                name: service.display_name().to_string(),
                status: if healthy { "running" } else { "stopped" }.to_string(),
                pid: None,
                port: ProcessManager::port_for_service(service, &self.config),
                uptime_secs: None,
                version,
            });
        }
        let is_ready = services.iter().all(|s| s.status == "running");

        let mut status = SystemStatus {
            network: NETWORK_ID.to_string(),
            bitcoin_network: BITCOIN_NETWORK_ID.to_string(),
            services,
            block_height: 0,
            mempool_size: 0,
            is_ready,
        };

        let qubitcoind_running = status
            .services
            .iter()
            .any(|s| s.id == "qubitcoind" && s.status == "running");
        if qubitcoind_running {
            if let Some(height) = crate::rpc::try_block_count(&self.config).await {
                status.block_height = height;
            }
            if let Some(size) = crate::rpc::try_mempool_size(&self.config).await {
                status.mempool_size = size;
            }
        }

        status
    }

    /// Mine blocks (to the default dev address unless one is given).
    pub async fn mine(&self, count: u32, address: Option<String>) -> Result<u64, String> {
        let addr = match address {
            Some(address) => address,
            None => crate::faucet::address()?,
        };
        crate::rpc::mine_blocks(&self.config, count, &addr).await
    }

    /// Construct, sign, and broadcast an exact-amount faucet transaction.
    pub async fn fund(&self, address: &str, amount: f64) -> Result<String, String> {
        crate::faucet::fund(&self.config, address, amount).await
    }

    /// Recent service logs (most recent `limit`, optionally one service).
    ///
    /// Reads the `logs/<service>.log` files written by file-mode process
    /// managers (i.e. services started by `labcoat up`), falling back to
    /// this handle's in-memory buffer for anything it spawned itself.
    pub fn logs(&self, service: Option<String>, limit: usize) -> Vec<LogEntry> {
        let mut entries: Vec<LogEntry> = Vec::new();

        let services: Vec<ServiceId> = match &service {
            Some(name) => ServiceId::all()
                .into_iter()
                .filter(|s| s.id() == name)
                .collect(),
            None => ServiceId::all(),
        };

        for svc in services {
            let path = get_logs_dir().join(format!("{}.log", svc.id()));
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            let lines: Vec<&str> = content.lines().collect();
            let start = lines.len().saturating_sub(limit);
            for line in &lines[start..] {
                entries.push(LogEntry {
                    service: svc.id().to_string(),
                    timestamp: 0, // file logs carry the services' own timestamps
                    message: line.to_string(),
                    is_stderr: false,
                });
            }
        }

        if entries.is_empty() {
            entries = self.process_manager.get_logs(service, limit);
        }

        // Cap the combined view
        let start = entries.len().saturating_sub(limit);
        entries[start..].to_vec()
    }

    /// The one public runtime endpoint.
    pub fn endpoints(&self) -> serde_json::Value {
        let p = &self.config.ports;
        serde_json::json!({
            "qubitcoin_rpc": format!("http://127.0.0.1:{}", p.qubitcoin_rpc),
        })
    }

    fn snapshots_dir() -> PathBuf {
        crate::config::get_data_dir().join("snapshots-v2")
    }

    /// Snapshot the Labcoat Network data directory under the given name.
    /// Services must be stopped first (call [`LabcoatNetwork::stop`]).
    pub fn snapshot(&mut self, name: &str) -> Result<PathBuf, String> {
        validate_snapshot_name(name)?;
        let src = get_runtime_dir();
        if !src.exists() {
            return Err(
                "No Labcoat Network data to snapshot (data directory is empty)".to_string(),
            );
        }
        let dest = Self::snapshots_dir().join(name);
        if dest.exists() {
            return Err(format!("Snapshot '{name}' already exists"));
        }
        self.stop()?;
        copy_dir(&src, &dest).map_err(|e| format!("Snapshot failed: {e}"))?;
        Ok(dest)
    }

    /// Replace the Labcoat Network data directory with a snapshot.
    /// Services are stopped; restart with [`LabcoatNetwork::start`].
    pub fn restore(&mut self, name: &str) -> Result<(), String> {
        validate_snapshot_name(name)?;
        let src = Self::snapshots_dir().join(name);
        if !src.exists() {
            return Err(format!("Snapshot '{name}' not found"));
        }
        self.stop()?;
        let data = get_runtime_dir();
        if data.exists() {
            std::fs::remove_dir_all(&data).map_err(|e| format!("Restore failed: {e}"))?;
        }
        copy_dir(&src, &data).map_err(|e| format!("Restore failed: {e}"))?;
        Ok(())
    }

    /// List snapshot names.
    pub fn snapshots(&self) -> Vec<String> {
        let dir = Self::snapshots_dir();
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .flatten()
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
            .collect();
        names.sort();
        names
    }
}

impl Default for LabcoatNetwork {
    fn default() -> Self {
        Self::new()
    }
}

fn validate_snapshot_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("Snapshot names must be non-empty [a-zA-Z0-9_-]".to_string());
    }
    Ok(())
}

fn copy_dir(src: &PathBuf, dest: &PathBuf) -> std::io::Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let to = dest.join(entry.file_name());
        if ty.is_dir() {
            copy_dir(&entry.path(), &to)?;
        } else if ty.is_file() {
            std::fs::copy(entry.path(), &to)?;
        }
        // symlinks are skipped (none are expected in Labcoat Network data dirs)
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_names_are_validated() {
        assert!(validate_snapshot_name("ok-name_1").is_ok());
        assert!(validate_snapshot_name("").is_err());
        assert!(validate_snapshot_name("../escape").is_err());
        assert!(validate_snapshot_name("a/b").is_err());
    }

    #[test]
    fn endpoint_manifest_exposes_only_qubitcoin() {
        let endpoints = LabcoatNetwork::with_config(IsomerConfig::default()).endpoints();
        assert_eq!(
            endpoints,
            serde_json::json!({"qubitcoin_rpc": "http://127.0.0.1:18443"})
        );
    }

    #[test]
    fn identity_distinguishes_target_from_bitcoin_mode() {
        assert_eq!(NETWORK_ID, "labcoat");
        assert_eq!(BITCOIN_NETWORK_ID, "regtest");
    }
}
