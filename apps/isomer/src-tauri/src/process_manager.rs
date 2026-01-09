//! Process management for Isomer services
//!
//! Handles spawning, monitoring, and graceful shutdown of all child processes

use crate::config::{get_bin_dir, get_logs_dir, get_runtime_dir, IsomerConfig};
use crate::state::{ServiceInfo, ServiceStatus};
use reqwest;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Instant;

/// Service identifiers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceId {
    Bitcoind,
    Metashrew,
    Memshrew,
    Ord,
    Esplora,
    JsonRpc,
}

impl ServiceId {
    pub fn all() -> Vec<ServiceId> {
        vec![
            ServiceId::Bitcoind,
            ServiceId::Metashrew,
            ServiceId::Memshrew,
            ServiceId::Ord,
            ServiceId::Esplora,
            ServiceId::JsonRpc,
        ]
    }

    pub fn name(&self) -> &'static str {
        match self {
            ServiceId::Bitcoind => "bitcoind",
            ServiceId::Metashrew => "metashrew",
            ServiceId::Memshrew => "memshrew",
            ServiceId::Ord => "ord",
            ServiceId::Esplora => "esplora",
            ServiceId::JsonRpc => "jsonrpc",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            ServiceId::Bitcoind => "Bitcoin Core",
            ServiceId::Metashrew => "Metashrew (Indexer)",
            ServiceId::Memshrew => "Memshrew (Mempool)",
            ServiceId::Ord => "Ord (Inscriptions)",
            ServiceId::Esplora => "Esplora (Explorer)",
            ServiceId::JsonRpc => "Alkanes JSON-RPC",
        }
    }

    pub fn binary_name(&self) -> &'static str {
        match self {
            ServiceId::Bitcoind => "bitcoind",
            ServiceId::Metashrew => "rockshrew-mono",
            ServiceId::Memshrew => "memshrew-p2p",
            ServiceId::Ord => "ord",
            ServiceId::Esplora => "flextrs",
            ServiceId::JsonRpc => "jsonrpc",
        }
    }

    /// Get startup dependencies (services that must be running first)
    pub fn dependencies(&self) -> Vec<ServiceId> {
        match self {
            ServiceId::Bitcoind => vec![],
            ServiceId::Metashrew => vec![ServiceId::Bitcoind],
            ServiceId::Memshrew => vec![ServiceId::Bitcoind],
            ServiceId::Ord => vec![ServiceId::Bitcoind],
            ServiceId::Esplora => vec![ServiceId::Bitcoind],
            ServiceId::JsonRpc => vec![
                ServiceId::Bitcoind,
                ServiceId::Metashrew,
                ServiceId::Memshrew,
                ServiceId::Ord,
                ServiceId::Esplora,
            ],
        }
    }
}

/// Information about a running process
struct ProcessInfo {
    child: Child,
    started_at: Instant,
    status: ServiceStatus,
}

/// Manages all Isomer child processes
pub struct ProcessManager {
    processes: HashMap<ServiceId, ProcessInfo>,
}

impl ProcessManager {
    pub fn new() -> Self {
        Self {
            processes: HashMap::new(),
        }
    }

    /// Get the binary path for a service
    fn get_binary_path(&self, service: ServiceId) -> PathBuf {
        get_bin_dir().join(service.binary_name())
    }

    /// Build the command arguments for a service
    fn build_args(&self, service: ServiceId, config: &IsomerConfig) -> Vec<String> {
        let ports = &config.ports;
        let btc = &config.bitcoind;

        match service {
            ServiceId::Bitcoind => vec![
                "-txindex".to_string(),
                "-regtest=1".to_string(),
                "-printtoconsole".to_string(),
                "-rpcallowip=0.0.0.0/0".to_string(),
                "-rpcbind=0.0.0.0".to_string(),
                format!("-rpcport={}", ports.bitcoind_rpc),
                format!("-port={}", ports.bitcoind_p2p),
                format!("-rpcuser={}", btc.rpc_user),
                format!("-rpcpassword={}", btc.rpc_password),
                format!("-fallbackfee={}", btc.fallback_fee),
                format!("-datadir={}", get_runtime_dir().join("bitcoin").display()),
            ],
            ServiceId::Metashrew => vec![
                "--host".to_string(),
                "0.0.0.0".to_string(),
                "--port".to_string(),
                ports.metashrew.to_string(),
                "--indexer".to_string(),
                get_bin_dir().join("alkanes.wasm").display().to_string(),
                "--db-path".to_string(),
                get_runtime_dir().join("metashrew").display().to_string(),
                "--auth".to_string(),
                format!("{}:{}", btc.rpc_user, btc.rpc_password),
                "--daemon-rpc-url".to_string(),
                format!("http://127.0.0.1:{}", ports.bitcoind_rpc),
            ],
            ServiceId::Memshrew => vec![
                "--daemon-rpc-url".to_string(),
                format!("http://127.0.0.1:{}", ports.bitcoind_rpc),
                "--p2p-addr".to_string(),
                format!("127.0.0.1:{}", ports.bitcoind_p2p),
                "--auth".to_string(),
                format!("{}:{}", btc.rpc_user, btc.rpc_password),
                "--host".to_string(),
                "0.0.0.0".to_string(),
                "--port".to_string(),
                ports.memshrew.to_string(),
            ],
            ServiceId::Ord => vec![
                "--index-transactions".to_string(),
                "--index-addresses".to_string(),
                "--index-sats".to_string(),
                "--index-runes".to_string(),
                "--chain".to_string(),
                "regtest".to_string(),
                "--bitcoin-rpc-url".to_string(),
                format!("127.0.0.1:{}", ports.bitcoind_rpc),
                "--bitcoin-rpc-username".to_string(),
                btc.rpc_user.clone(),
                "--bitcoin-rpc-password".to_string(),
                btc.rpc_password.clone(),
                "--bitcoin-data-dir".to_string(),
                get_runtime_dir().join("bitcoin").display().to_string(),
                "server".to_string(),
                "--http-port".to_string(),
                ports.ord.to_string(),
            ],
            ServiceId::Esplora => vec![
                "-vvv".to_string(),
                "--db-dir".to_string(),
                get_runtime_dir().join("esplora").display().to_string(),
                "--daemon-dir".to_string(),
                get_runtime_dir().join("bitcoin").display().to_string(),
                "--network".to_string(),
                "regtest".to_string(),
                "--daemon-rpc-addr".to_string(),
                format!("127.0.0.1:{}", ports.bitcoind_rpc),
                "--http-addr".to_string(),
                format!("0.0.0.0:{}", ports.esplora_http),
                "--electrum-rpc-addr".to_string(),
                format!("0.0.0.0:{}", ports.esplora_electrum),
                "--auth".to_string(),
                format!("{}:{}", btc.rpc_user, btc.rpc_password),
            ],
            ServiceId::JsonRpc => vec![get_bin_dir()
                .join("jsonrpc/bin/jsonrpc")
                .display()
                .to_string()],
        }
    }

    /// Build environment variables for a service
    fn build_env(&self, service: ServiceId, config: &IsomerConfig) -> HashMap<String, String> {
        let mut env = HashMap::new();
        let ports = &config.ports;
        let btc = &config.bitcoind;

        if service == ServiceId::JsonRpc {
            env.insert("SERVER_HOST".to_string(), "0.0.0.0".to_string());
            env.insert("SERVER_PORT".to_string(), ports.jsonrpc.to_string());
            env.insert(
                "BITCOIN_RPC_URL".to_string(),
                format!("http://127.0.0.1:{}", ports.bitcoind_rpc),
            );
            env.insert("BITCOIN_RPC_USER".to_string(), btc.rpc_user.clone());
            env.insert("BITCOIN_RPC_PASSWORD".to_string(), btc.rpc_password.clone());
            env.insert(
                "METASHREW_URL".to_string(),
                format!("http://127.0.0.1:{}", ports.metashrew),
            );
            env.insert(
                "MEMSHREW_URL".to_string(),
                format!("http://127.0.0.1:{}", ports.memshrew),
            );
            env.insert(
                "ORD_URL".to_string(),
                format!("http://127.0.0.1:{}", ports.ord),
            );
            env.insert(
                "ESPLORA_URL".to_string(),
                format!("http://127.0.0.1:{}", ports.esplora_http),
            );
            env.insert("RUST_LOG".to_string(), "info".to_string());
        }

        env
    }

    /// Start a single service
    pub fn start_service(
        &mut self,
        service: ServiceId,
        config: &IsomerConfig,
    ) -> Result<(), String> {
        if self.processes.contains_key(&service) {
            return Err(format!("{} is already running", service.display_name()));
        }

        let binary_path = self.get_binary_path(service);
        if !binary_path.exists() {
            return Err(format!(
                "Binary not found: {}. Please download binaries first.",
                binary_path.display()
            ));
        }

        // Ensure data directories exist
        let _ = std::fs::create_dir_all(get_runtime_dir().join("bitcoin"));
        let _ = std::fs::create_dir_all(get_runtime_dir().join("metashrew"));
        let _ = std::fs::create_dir_all(get_runtime_dir().join("esplora"));
        let _ = std::fs::create_dir_all(get_logs_dir());

        let args = self.build_args(service, config);
        let env = self.build_env(service, config);

        tracing::info!("Starting {} with args: {:?}", service.display_name(), args);

        let mut cmd = if service == ServiceId::JsonRpc {
            // For JsonRpc, we expect 'node' to be in the PATH
            Command::new("node")
        } else {
            Command::new(&binary_path)
        };

        cmd.args(&args)
            .envs(&env)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        match cmd.spawn() {
            Ok(child) => {
                let pid = child.id();
                self.processes.insert(
                    service,
                    ProcessInfo {
                        child,
                        started_at: Instant::now(),
                        status: ServiceStatus::Starting,
                    },
                );
                tracing::info!("{} started with PID {}", service.display_name(), pid);
                Ok(())
            }
            Err(e) => Err(format!("Failed to start {}: {}", service.display_name(), e)),
        }
    }

    /// Stop a single service
    pub fn stop_service(&mut self, service: ServiceId) -> Result<(), String> {
        if let Some(mut info) = self.processes.remove(&service) {
            tracing::info!("Stopping {}", service.display_name());

            // Try graceful shutdown first (SIGTERM on Unix)
            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt;
                let pid = info.child.id();
                unsafe {
                    libc::kill(pid as i32, libc::SIGTERM);
                }
            }

            #[cfg(windows)]
            {
                let _ = info.child.kill();
            }

            // Wait for process to exit
            match info.child.wait() {
                Ok(status) => {
                    tracing::info!("{} stopped: {:?}", service.display_name(), status);
                    Ok(())
                }
                Err(e) => Err(format!(
                    "Error waiting for {}: {}",
                    service.display_name(),
                    e
                )),
            }
        } else {
            Ok(()) // Already stopped
        }
    }

    /// Start all services in dependency order
    pub fn start_all(&mut self, config: &IsomerConfig) -> Result<(), String> {
        let order = vec![
            ServiceId::Bitcoind,
            ServiceId::Metashrew,
            ServiceId::Memshrew,
            ServiceId::Ord,
            ServiceId::Esplora,
            ServiceId::JsonRpc,
        ];

        for service in order {
            self.start_service(service, config)?;

            // Wait a bit between services for stability
            std::thread::sleep(std::time::Duration::from_millis(500));
        }

        Ok(())
    }

    /// Stop all services in reverse dependency order
    pub fn stop_all(&mut self) -> Result<(), String> {
        let order = vec![
            ServiceId::JsonRpc,
            ServiceId::Esplora,
            ServiceId::Ord,
            ServiceId::Memshrew,
            ServiceId::Metashrew,
            ServiceId::Bitcoind,
        ];

        for service in order {
            self.stop_service(service)?;
        }

        Ok(())
    }

    /// Get status of all services
    pub fn get_all_status(&mut self) -> Vec<ServiceInfo> {
        ServiceId::all()
            .into_iter()
            .map(|id| self.get_service_info(id))
            .collect()
    }

    /// Get info about a specific service
    fn get_service_info(&mut self, service: ServiceId) -> ServiceInfo {
        let (status, pid, uptime) = if let Some(info) = self.processes.get_mut(&service) {
            // Check if process is still running
            match info.child.try_wait() {
                Ok(Some(exit_status)) => {
                    // Process has exited
                    let status = if exit_status.success() {
                        ServiceStatus::Stopped
                    } else {
                        ServiceStatus::Error(format!("Exited with code: {:?}", exit_status.code()))
                    };
                    (status, None, None)
                }
                Ok(None) => {
                    // Process is still running
                    (
                        ServiceStatus::Running,
                        Some(info.child.id()),
                        Some(info.started_at.elapsed().as_secs()),
                    )
                }
                Err(e) => {
                    // Error checking status
                    (
                        ServiceStatus::Error(format!("Status check failed: {}", e)),
                        None,
                        None,
                    )
                }
            }
        } else {
            (ServiceStatus::Stopped, None, None)
        };

        ServiceInfo {
            id: service.name().to_string(),
            name: service.display_name().to_string(),
            status,
            port: self.get_port_for_service(service),
            pid,
            uptime_secs: uptime,
        }
    }

    fn get_port_for_service(&self, service: ServiceId) -> u16 {
        // Return default ports (actual config would be accessed differently)
        match service {
            ServiceId::Bitcoind => 18443,
            ServiceId::Metashrew => 8080,
            ServiceId::Memshrew => 8081,
            ServiceId::Ord => 8090,
            ServiceId::Esplora => 50010,
            ServiceId::JsonRpc => 18888,
        }
    }

    /// Check if a service is healthy (responding to HTTP/RPC)
    pub async fn check_health(&self, service: ServiceId, config: &IsomerConfig) -> bool {
        // First check if process is running
        if !self.processes.contains_key(&service) {
            return false;
        }

        let ports = &config.ports;
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .unwrap_or_default();

        let url = match service {
            ServiceId::Bitcoind => format!("http://127.0.0.1:{}", ports.bitcoind_rpc),
            ServiceId::Metashrew => format!("http://127.0.0.1:{}", ports.metashrew),
            ServiceId::Memshrew => format!("http://127.0.0.1:{}", ports.memshrew),
            ServiceId::Ord => format!("http://127.0.0.1:{}/status", ports.ord),
            ServiceId::Esplora => {
                format!("http://127.0.0.1:{}/blocks/tip/height", ports.esplora_http)
            }
            ServiceId::JsonRpc => format!("http://127.0.0.1:{}", ports.jsonrpc),
        };

        match client.get(&url).send().await {
            Ok(res) => {
                // Accept success (2xx) or Unauthorized (401) as sign of life
                // Some services might return 404 or 405 for root path but still be running
                res.status().is_success()
                    || res.status() == reqwest::StatusCode::UNAUTHORIZED
                    || res.status() == reqwest::StatusCode::METHOD_NOT_ALLOWED
                    || res.status() == reqwest::StatusCode::NOT_FOUND
            }
            Err(_) => false,
        }
    }
}

impl Drop for ProcessManager {
    fn drop(&mut self) {
        // Ensure all processes are stopped when Isomer exits
        let _ = self.stop_all();
    }
}
