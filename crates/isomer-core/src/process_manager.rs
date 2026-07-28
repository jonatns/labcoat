//! Qubitcoin process management for the Labcoat devnet.

use crate::config::{get_bin_dir, get_logs_dir, get_qubitcoin_dir, IsomerConfig};
use crate::state::{ServiceInfo, ServiceStatus};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceId {
    Qubitcoind,
}

impl ServiceId {
    pub fn all() -> Vec<Self> {
        vec![Self::Qubitcoind]
    }

    pub fn name(self) -> &'static str {
        "qubitcoind"
    }

    pub fn id(self) -> &'static str {
        self.name()
    }

    pub fn display_name(self) -> &'static str {
        "Qubitcoin"
    }

    pub fn binary_name(self) -> &'static str {
        "qubitcoind"
    }

    pub fn dependencies(self) -> Vec<Self> {
        Vec::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub service: String,
    pub timestamp: u64,
    pub message: String,
    pub is_stderr: bool,
}

struct ProcessInfo {
    child: Child,
    started_at: Instant,
}

type LogBuffer = std::sync::Arc<std::sync::Mutex<Vec<LogEntry>>>;
const MAX_LOG_ENTRIES: usize = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogMode {
    Pipe,
    File,
}

pub struct ProcessManager {
    processes: HashMap<ServiceId, ProcessInfo>,
    log_buffer: LogBuffer,
    log_mode: LogMode,
}

impl Default for ProcessManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessManager {
    pub fn new() -> Self {
        Self::kill_orphans();
        Self::with_options(LogMode::Pipe)
    }

    pub fn detached() -> Self {
        Self::with_options(LogMode::File)
    }

    fn with_options(log_mode: LogMode) -> Self {
        Self {
            processes: HashMap::new(),
            log_buffer: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            log_mode,
        }
    }

    pub fn kill_orphans() {
        #[cfg(unix)]
        {
            let _ = Command::new("pkill").args(["-x", "qubitcoind"]).output();
            for port in [18443_u16, 18444_u16] {
                if let Ok(output) = Command::new("lsof")
                    .args(["-t", &format!("-i:{port}")])
                    .output()
                {
                    for pid in String::from_utf8_lossy(&output.stdout).lines() {
                        if !pid.trim().is_empty() {
                            let _ = Command::new("kill").args(["-9", pid]).output();
                        }
                    }
                }
            }
        }

        #[cfg(windows)]
        {
            let _ = Command::new("taskkill")
                .args(["/F", "/IM", "qubitcoind.exe"])
                .output();
        }
    }

    pub fn get_logs(&self, service_filter: Option<String>, limit: usize) -> Vec<LogEntry> {
        let logs = self.log_buffer.lock().unwrap();
        let filtered: Vec<_> = logs
            .iter()
            .filter(|entry| {
                service_filter
                    .as_ref()
                    .is_none_or(|filter| entry.service == *filter)
            })
            .cloned()
            .collect();
        let start = filtered.len().saturating_sub(limit);
        filtered[start..].to_vec()
    }

    pub fn clear_logs(&self) {
        self.log_buffer.lock().unwrap().clear();
    }

    fn add_log_entry(buffer: &LogBuffer, entry: LogEntry) {
        let mut logs = buffer.lock().unwrap();
        logs.push(entry);
        if logs.len() > MAX_LOG_ENTRIES {
            let excess = logs.len() - MAX_LOG_ENTRIES;
            logs.drain(0..excess);
        }
    }

    fn build_args(config: &IsomerConfig) -> Vec<String> {
        vec![
            "-regtest=1".into(),
            "-rpcbind=127.0.0.1".into(),
            format!("-rpcport={}", config.ports.qubitcoin_rpc),
            format!("-port={}", config.ports.qubitcoin_p2p),
            format!("-datadir={}", get_qubitcoin_dir().display()),
            "-synchronous-secondary=1".into(),
            format!(
                "-loadindexer=alkanes:{}",
                get_bin_dir().join("alkanes.wasm").display()
            ),
            format!(
                "-loadindexer=esplora:{}",
                get_bin_dir().join("esplorashrew.wasm").display()
            ),
        ]
    }

    pub fn start_service(
        &mut self,
        service: ServiceId,
        config: &IsomerConfig,
    ) -> Result<(), String> {
        if self.processes.contains_key(&service) {
            return Err(format!("{} is already running", service.display_name()));
        }

        let binary = get_bin_dir().join(service.binary_name());
        if !binary.exists() {
            return Err(format!(
                "Binary not found: {}. Please download binaries first.",
                binary.display()
            ));
        }
        for asset in ["alkanes.wasm", "esplorashrew.wasm"] {
            let path = get_bin_dir().join(asset);
            if !path.exists() {
                return Err(format!("Runtime asset not found: {}", path.display()));
            }
        }

        std::fs::create_dir_all(get_qubitcoin_dir())
            .map_err(|e| format!("Failed to create Qubitcoin data directory: {e}"))?;
        std::fs::create_dir_all(get_logs_dir())
            .map_err(|e| format!("Failed to create log directory: {e}"))?;

        let mut command = Command::new(&binary);
        command
            .args(Self::build_args(config))
            .env("RUST_LOG", "info");

        match self.log_mode {
            LogMode::Pipe => {
                command.stdout(Stdio::piped()).stderr(Stdio::piped());
            }
            LogMode::File => {
                let log_path = get_logs_dir().join("qubitcoind.log");
                let open = || {
                    std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&log_path)
                };
                match (open(), open()) {
                    (Ok(stdout), Ok(stderr)) => {
                        command
                            .stdout(Stdio::from(stdout))
                            .stderr(Stdio::from(stderr));
                    }
                    _ => {
                        command.stdout(Stdio::null()).stderr(Stdio::null());
                    }
                }
            }
        }

        let mut child = command
            .spawn()
            .map_err(|e| format!("Failed to start {}: {e}", service.display_name()))?;
        let pid = child.id();

        if let Some(stdout) = child.stdout.take() {
            Self::capture_logs(stdout, self.log_buffer.clone(), false);
        }
        if let Some(stderr) = child.stderr.take() {
            Self::capture_logs(stderr, self.log_buffer.clone(), true);
        }

        self.processes.insert(
            service,
            ProcessInfo {
                child,
                started_at: Instant::now(),
            },
        );
        tracing::info!("{} started with PID {}", service.display_name(), pid);
        Ok(())
    }

    fn capture_logs(
        stream: impl std::io::Read + Send + 'static,
        buffer: LogBuffer,
        is_stderr: bool,
    ) {
        std::thread::spawn(move || {
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                Self::add_log_entry(
                    &buffer,
                    LogEntry {
                        service: "qubitcoind".into(),
                        timestamp: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                        message: line,
                        is_stderr,
                    },
                );
            }
        });
    }

    pub fn stop_service(&mut self, service: ServiceId) -> Result<(), String> {
        let Some(mut info) = self.processes.remove(&service) else {
            return Ok(());
        };

        #[cfg(unix)]
        unsafe {
            libc::kill(info.child.id() as i32, libc::SIGTERM);
        }
        #[cfg(windows)]
        let _ = info.child.kill();

        std::thread::sleep(std::time::Duration::from_millis(500));
        if info.child.try_wait().ok().flatten().is_none() {
            let _ = info.child.kill();
        }
        info.child
            .wait()
            .map(|_| ())
            .map_err(|e| format!("Error waiting for {}: {e}", service.display_name()))
    }

    pub fn start_all(&mut self, config: &IsomerConfig) -> Result<(), String> {
        self.start_service(ServiceId::Qubitcoind, config)
    }

    pub fn stop_all(&mut self) -> Result<(), String> {
        self.stop_service(ServiceId::Qubitcoind)
    }

    pub fn reset_data(&mut self) -> Result<(), String> {
        let _ = self.stop_all();
        Self::kill_orphans();
        let dir = get_qubitcoin_dir();
        if dir.exists() {
            std::fs::remove_dir_all(&dir)
                .map_err(|e| format!("Failed to remove {}: {e}", dir.display()))?;
        }
        Ok(())
    }

    pub fn get_all_status(&mut self, config: &IsomerConfig) -> Vec<ServiceInfo> {
        ServiceId::all()
            .into_iter()
            .map(|service| self.get_service_info(service, config))
            .collect()
    }

    fn get_service_info(&mut self, service: ServiceId, config: &IsomerConfig) -> ServiceInfo {
        let (status, pid, uptime) = match self.processes.get_mut(&service) {
            Some(info) => match info.child.try_wait() {
                Ok(None) => (
                    ServiceStatus::Running,
                    Some(info.child.id()),
                    Some(info.started_at.elapsed().as_secs()),
                ),
                Ok(Some(exit)) => (
                    ServiceStatus::Error(format!("Exited with code: {:?}", exit.code())),
                    None,
                    None,
                ),
                Err(error) => (ServiceStatus::Error(error.to_string()), None, None),
            },
            None => (ServiceStatus::Stopped, None, None),
        };
        let status = match status {
            ServiceStatus::Stopped => "stopped",
            ServiceStatus::Starting => "starting",
            ServiceStatus::Running => "running",
            ServiceStatus::Error(_) => "error",
        };
        let version = crate::binary_manager::BinaryManager::new()
            .check_binary(service)
            .status
            .into_version();
        ServiceInfo {
            id: service.id().into(),
            name: service.display_name().into(),
            status: status.into(),
            pid,
            port: Self::port_for_service(service, config),
            uptime_secs: uptime,
            version,
        }
    }

    pub fn port_for_service(_service: ServiceId, config: &IsomerConfig) -> u16 {
        config.ports.qubitcoin_rpc
    }

    pub async fn check_health(&self, service: ServiceId, config: &IsomerConfig) -> bool {
        self.processes.contains_key(&service) && Self::probe_health(service, config).await
    }

    pub async fn probe_health(_service: ServiceId, config: &IsomerConfig) -> bool {
        crate::rpc::call(
            config,
            "getblockchaininfo",
            serde_json::json!([]),
            std::time::Duration::from_secs(2),
        )
        .await
        .is_ok()
    }
}

impl Drop for ProcessManager {
    fn drop(&mut self) {
        let _ = self.stop_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qubitcoin_args_load_both_indexers_synchronously() {
        let args = ProcessManager::build_args(&IsomerConfig::default());
        assert!(args.contains(&"-synchronous-secondary=1".to_string()));
        assert!(args
            .iter()
            .any(|arg| arg.starts_with("-loadindexer=alkanes:")));
        assert!(args
            .iter()
            .any(|arg| arg.starts_with("-loadindexer=esplora:")));
        assert_eq!(
            args.iter()
                .filter(|arg| arg.starts_with("-loadindexer="))
                .count(),
            2
        );
    }

    #[test]
    fn runtime_has_one_service() {
        assert_eq!(ServiceId::all(), vec![ServiceId::Qubitcoind]);
        assert_eq!(ServiceId::Qubitcoind.id(), "qubitcoind");
    }
}
