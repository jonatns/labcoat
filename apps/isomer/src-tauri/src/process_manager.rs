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

    /// Get the lowercase ID for the service (used for logging/filtering)
    pub fn id(&self) -> &'static str {
        match self {
            ServiceId::Bitcoind => "bitcoind",
            ServiceId::Metashrew => "metashrew",
            ServiceId::Memshrew => "memshrew",
            ServiceId::Ord => "ord",
            ServiceId::Esplora => "esplora",
            ServiceId::JsonRpc => "alkanes-jsonrpc",
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

/// A single log entry from a service
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub service: String,
    pub timestamp: u64,
    pub message: String,
    pub is_stderr: bool,
}

/// Information about a running process
struct ProcessInfo {
    child: Child,
    started_at: Instant,
    status: ServiceStatus,
}

/// Shared log buffer type
type LogBuffer = std::sync::Arc<std::sync::Mutex<Vec<LogEntry>>>;

/// Maximum number of log entries to keep
const MAX_LOG_ENTRIES: usize = 1000;

/// Manages all Isomer child processes
pub struct ProcessManager {
    processes: HashMap<ServiceId, ProcessInfo>,
    /// Shared log buffer captured from all services
    log_buffer: LogBuffer,
}

impl ProcessManager {
    pub fn new() -> Self {
        // Clean up any orphaned processes from previous runs
        Self::kill_orphans();

        Self {
            processes: HashMap::new(),
            log_buffer: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    /// Kill any existing processes that might be orphaned
    fn kill_orphans() {
        tracing::info!("Cleaning up orphaned processes...");

        // 1. Kill by name
        let binaries = vec![
            "bitcoind",
            "rockshrew-mono",
            "memshrew-p2p",
            "ord",
            "flextrs",
            "jsonrpc",
        ];

        #[cfg(unix)]
        {
            for binary in &binaries {
                // Remove -x to be more aggressive, match substring if needed, but risky.
                // Keeping -x but ensuring it works, maybe use "pgrep" first to log.
                let _ = Command::new("pkill").arg("-x").arg(binary).output();
            }
        }

        #[cfg(windows)]
        {
            for binary in &binaries {
                let _ = Command::new("taskkill")
                    .arg("/F")
                    .arg("/IM")
                    .arg(format!("{}.exe", binary))
                    .output();
            }
        }

        // 2. Kill by port (The nuclear option for rogue processes)
        // Default ports to check (hardcoded here for safety, though ideally from config)
        let ports = vec![
            18443, // bitcoind rpc
            18444, // bitcoind p2p
            8080,  // metashrew
            8081,  // memshrew
            8090,  // ord
            50010, // esplora http
            18888, // jsonrpc
        ];

        #[cfg(unix)]
        for port in ports {
            // lsof -t -i:PORT returns just the PID
            if let Ok(output) = Command::new("lsof")
                .arg("-t")
                .arg(format!("-i:{}", port))
                .output()
            {
                if let Ok(pids_str) = String::from_utf8(output.stdout) {
                    for pid_str in pids_str.lines() {
                        if !pid_str.trim().is_empty() {
                            tracing::warn!(
                                "Port {} occupied by PID {}. Force killing...",
                                port,
                                pid_str
                            );
                            let _ = Command::new("kill").arg("-9").arg(pid_str).output();
                        }
                    }
                }
            }
        }

        // Give OS a moment to reclaim resources
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    /// Get recent logs, optionally filtered by service
    pub fn get_logs(&self, service_filter: Option<String>, limit: usize) -> Vec<LogEntry> {
        let logs = self.log_buffer.lock().unwrap();
        let filtered: Vec<LogEntry> = if let Some(ref filter) = service_filter {
            logs.iter()
                .filter(|l| l.service == *filter)
                .cloned()
                .collect()
        } else {
            logs.clone()
        };

        // Return last N entries
        let start = filtered.len().saturating_sub(limit);
        filtered[start..].to_vec()
    }

    /// Clear all logs
    pub fn clear_logs(&self) {
        let mut logs = self.log_buffer.lock().unwrap();
        logs.clear();
    }

    /// Add a log entry (called from log reader threads)
    fn add_log_entry(buffer: &LogBuffer, entry: LogEntry) {
        let mut logs = buffer.lock().unwrap();
        logs.push(entry);
        // Keep only the last MAX_LOG_ENTRIES
        if logs.len() > MAX_LOG_ENTRIES {
            let excess = logs.len() - MAX_LOG_ENTRIES;
            logs.drain(0..excess);
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
                "--data-dir".to_string(),
                get_runtime_dir().join("ord").display().to_string(),
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
                .join("jsonrpc/bin/jsonrpc.js")
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
            // Note: alkanes-jsonrpc expects these specific env var names
            env.insert("HOST".to_string(), "0.0.0.0".to_string());
            env.insert("PORT".to_string(), ports.jsonrpc.to_string());
            env.insert(
                "DAEMON_RPC_ADDR".to_string(),
                format!("127.0.0.1:{}", ports.bitcoind_rpc),
            );
            env.insert("RPCUSER".to_string(), btc.rpc_user.clone());
            env.insert("RPCPASSWORD".to_string(), btc.rpc_password.clone());
            env.insert(
                "METASHREW_URI".to_string(),
                format!("http://127.0.0.1:{}", ports.metashrew),
            );
            env.insert(
                "MEMSHREW_URI".to_string(),
                format!("http://127.0.0.1:{}", ports.memshrew),
            );
            env.insert("ORD_HOST".to_string(), "127.0.0.1".to_string());
            env.insert("ORD_PORT".to_string(), ports.ord.to_string());
            env.insert("ESPLORA_HOST".to_string(), "127.0.0.1".to_string());
            env.insert("ESPLORA_PORT".to_string(), ports.esplora_http.to_string());
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
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        match cmd.spawn() {
            Ok(mut child) => {
                let pid = child.id();
                let service_name = service.id().to_string();

                // Capture stdout
                if let Some(stdout) = child.stdout.take() {
                    let buffer = self.log_buffer.clone();
                    let name = service_name.clone();
                    std::thread::spawn(move || {
                        use std::io::{BufRead, BufReader};
                        let reader = BufReader::new(stdout);
                        for line in reader.lines() {
                            if let Ok(line) = line {
                                // Also print to terminal for backward compatibility
                                println!("{}", line);

                                let entry = LogEntry {
                                    service: name.clone(),
                                    timestamp: std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_secs(),
                                    message: line,
                                    is_stderr: false,
                                };
                                Self::add_log_entry(&buffer, entry);
                            }
                        }
                    });
                }

                // Capture stderr
                if let Some(stderr) = child.stderr.take() {
                    let buffer = self.log_buffer.clone();
                    let name = service_name.clone();
                    std::thread::spawn(move || {
                        use std::io::{BufRead, BufReader};
                        let reader = BufReader::new(stderr);
                        for line in reader.lines() {
                            if let Ok(line) = line {
                                // Also print to terminal for backward compatibility
                                eprintln!("{}", line);

                                let entry = LogEntry {
                                    service: name.clone(),
                                    timestamp: std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_secs(),
                                    message: line,
                                    is_stderr: true,
                                };
                                Self::add_log_entry(&buffer, entry);
                            }
                        }
                    });
                }

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

                // Give it a moment to shut down gracefully
                std::thread::sleep(std::time::Duration::from_millis(500));

                // Force kill if still running
                unsafe {
                    libc::kill(pid as i32, libc::SIGKILL);
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

            // After bitcoind starts, wait longer and bootstrap wallet
            if service == ServiceId::Bitcoind {
                // Give bitcoind time to fully initialize
                std::thread::sleep(std::time::Duration::from_secs(2));

                // Bootstrap the wallet in a separate thread to avoid tokio runtime conflicts
                // (reqwest::blocking creates its own runtime which conflicts with Tauri's)
                let config_clone = config.clone();
                let handle = std::thread::spawn(move || Self::bootstrap_wallet_sync(&config_clone));

                match handle.join() {
                    Ok(Ok(())) => tracing::info!("Wallet bootstrap completed"),
                    Ok(Err(e)) => tracing::warn!("Wallet bootstrap warning: {}", e),
                    Err(_) => tracing::warn!("Wallet bootstrap thread panicked"),
                }
            } else {
                // Wait a bit between other services for stability
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        }

        Ok(())
    }

    /// Bootstrap the dev wallet - creates wallet and mines initial blocks if needed
    fn bootstrap_wallet_sync(config: &IsomerConfig) -> Result<(), String> {
        let rpc_url = format!("http://127.0.0.1:{}", config.ports.bitcoind_rpc);

        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

        // 1. Check if wallet exists, create if not
        let list_wallets: serde_json::Value = client
            .post(&rpc_url)
            .basic_auth(
                &config.bitcoind.rpc_user,
                Some(&config.bitcoind.rpc_password),
            )
            .json(&serde_json::json!({
                "jsonrpc": "1.0",
                "id": "isomer",
                "method": "listwallets",
                "params": []
            }))
            .send()
            .map_err(|e| format!("RPC call failed: {}", e))?
            .json()
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        let wallets = list_wallets
            .get("result")
            .and_then(|r| r.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
            .unwrap_or_default();

        if !wallets.contains(&"dev") {
            tracing::info!("Creating dev wallet...");
            let create_result: serde_json::Value = client
                .post(&rpc_url)
                .basic_auth(
                    &config.bitcoind.rpc_user,
                    Some(&config.bitcoind.rpc_password),
                )
                .json(&serde_json::json!({
                    "jsonrpc": "1.0",
                    "id": "isomer",
                    "method": "createwallet",
                    "params": ["dev"]
                }))
                .send()
                .map_err(|e| format!("Failed to create wallet: {}", e))?
                .json()
                .map_err(|e| format!("Failed to parse response: {}", e))?;

            if let Some(error) = create_result.get("error").and_then(|e| e.as_object()) {
                let code = error.get("code").and_then(|c| c.as_i64()).unwrap_or(0);
                // -4 means wallet already exists, which is fine
                if code != -4 {
                    return Err(format!("Failed to create wallet: {:?}", error));
                }
            }
            tracing::info!("Dev wallet created");
        } else {
            tracing::info!("Dev wallet already exists");
        }

        // 2. Load wallet if not loaded
        let _ = client
            .post(&rpc_url)
            .basic_auth(
                &config.bitcoind.rpc_user,
                Some(&config.bitcoind.rpc_password),
            )
            .json(&serde_json::json!({
                "jsonrpc": "1.0",
                "id": "isomer",
                "method": "loadwallet",
                "params": ["dev"]
            }))
            .send();

        // Use wallet-specific endpoint
        let wallet_rpc_url = format!("{}/wallet/dev", rpc_url);

        // 3. Get a new address or use existing
        let addr_result: serde_json::Value = client
            .post(&wallet_rpc_url)
            .basic_auth(
                &config.bitcoind.rpc_user,
                Some(&config.bitcoind.rpc_password),
            )
            .json(&serde_json::json!({
                "jsonrpc": "1.0",
                "id": "isomer",
                "method": "getnewaddress",
                "params": ["", "bech32m"]
            }))
            .send()
            .map_err(|e| format!("Failed to get address: {}", e))?
            .json()
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        let address = addr_result
            .get("result")
            .and_then(|r| r.as_str())
            .ok_or("Failed to get wallet address")?;

        tracing::info!("Dev wallet address: {}", address);

        // 4. Check current block height
        let height_result: serde_json::Value = client
            .post(&rpc_url)
            .basic_auth(
                &config.bitcoind.rpc_user,
                Some(&config.bitcoind.rpc_password),
            )
            .json(&serde_json::json!({
                "jsonrpc": "1.0",
                "id": "isomer",
                "method": "getblockcount",
                "params": []
            }))
            .send()
            .map_err(|e| format!("Failed to get block height: {}", e))?
            .json()
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        let current_height = height_result
            .get("result")
            .and_then(|r| r.as_u64())
            .unwrap_or(0);

        // 5. If chain is fresh (< 101 blocks), mine initial blocks for coinbase maturity
        if current_height < 101 {
            let blocks_to_mine = 101 - current_height as u32;
            tracing::info!("Mining {} blocks for coinbase maturity...", blocks_to_mine);

            let mine_result: serde_json::Value = client
                .post(&rpc_url)
                .basic_auth(
                    &config.bitcoind.rpc_user,
                    Some(&config.bitcoind.rpc_password),
                )
                .json(&serde_json::json!({
                    "jsonrpc": "1.0",
                    "id": "isomer",
                    "method": "generatetoaddress",
                    "params": [blocks_to_mine, address]
                }))
                .send()
                .map_err(|e| format!("Failed to mine blocks: {}", e))?
                .json()
                .map_err(|e| format!("Failed to parse response: {}", e))?;

            if let Some(error) = mine_result.get("error").and_then(|e| e.as_object()) {
                return Err(format!("Failed to mine blocks: {:?}", error));
            }

            tracing::info!("Mined {} blocks to {}", blocks_to_mine, address);
        } else {
            tracing::info!(
                "Chain already has {} blocks, skipping initial mining",
                current_height
            );
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

    /// Reset all data - stops services and clears data directories
    pub fn reset_data(&mut self) -> Result<(), String> {
        tracing::info!("Starting chain reset procedure...");

        // First, stop all services
        tracing::info!("Stopping all services...");
        if let Err(e) = self.stop_all() {
            tracing::warn!("Error stopping services (will attempt force kill): {}", e);
        }

        // Give processes time to fully terminate and release locks
        tracing::info!("Waiting for processes to terminate...");
        std::thread::sleep(std::time::Duration::from_secs(2));

        // Use our robust kill logic to ensure files aren't locked
        tracing::info!("Force killing any orphaned processes...");
        Self::kill_orphans();

        // Wait again after force kill
        std::thread::sleep(std::time::Duration::from_millis(500));

        // Clear data directories
        let data_dirs = vec![
            get_runtime_dir().join("bitcoin"),
            get_runtime_dir().join("metashrew"),
            get_runtime_dir().join("esplora"),
            get_runtime_dir().join("ord"),
        ];

        for dir in data_dirs {
            if dir.exists() {
                tracing::info!("Removing data directory: {}", dir.display());
                // Retry logic for directory deletion
                let mut attempts = 3;
                while attempts > 0 {
                    if let Err(e) = std::fs::remove_dir_all(&dir) {
                        tracing::warn!(
                            "Failed to remove {} (attempts left: {}): {}",
                            dir.display(),
                            attempts - 1,
                            e
                        );
                        std::thread::sleep(std::time::Duration::from_secs(1));
                        attempts -= 1;
                        if attempts == 0 {
                            return Err(format!(
                                "Failed to remove data directory {} after retries: {}",
                                dir.display(),
                                e
                            ));
                        }
                    } else {
                        tracing::info!("Successfully removed {}", dir.display());
                        break;
                    }
                }
            } else {
                tracing::info!(
                    "Data directory does not exist (skipping): {}",
                    dir.display()
                );
            }
        }

        tracing::info!("Chain data reset complete. Services are ready to restart.");
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

        // Get version from BinaryManager logic (re-using checking logic for now)
        // Ideally we'd cache this or pass BinaryManager, but for now we instantiate to check
        let version = crate::binary_manager::BinaryManager::new()
            .check_binary(service)
            .status
            .into_version();

        let status_str = match status {
            ServiceStatus::Stopped => "stopped",
            ServiceStatus::Starting => "starting",
            ServiceStatus::Running => "running",
            ServiceStatus::Error(_) => "error",
        }
        .to_string();

        let port = self.get_port_for_service(service);

        ServiceInfo {
            id: service.id().to_string(),
            name: service.display_name().to_string(),
            status: status_str,
            pid,
            port,
            uptime_secs: uptime,
            version,
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
