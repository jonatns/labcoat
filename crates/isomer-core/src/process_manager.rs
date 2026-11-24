//! The Labcoat Network process.
//!
//! Labcoat Network is served by the `labcoat` executable itself, run as
//! `labcoat node` in the background. It needs one runtime asset, the Alkanes
//! indexer module `alkanes.wasm`. `labcoat up` starts the process and it
//! outlives that command; later commands find it through its port and the
//! process id it records in the network data directory.

use crate::config::{get_bin_dir, get_compiled_dir, get_logs_dir, get_network_dir, IsomerConfig};
use crate::state::{ServiceInfo, ServiceStatus};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// The runtime asset the network needs.
pub const INDEXER_ASSET: &str = "alkanes.wasm";
/// The file in the network data directory holding the serving process id.
const PID_FILE: &str = "node.pid";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceId {
    Node,
}

impl ServiceId {
    pub fn all() -> Vec<Self> {
        vec![Self::Node]
    }

    pub fn name(self) -> &'static str {
        "node"
    }

    pub fn id(self) -> &'static str {
        self.name()
    }

    pub fn display_name(self) -> &'static str {
        "Labcoat Network"
    }

    /// The runtime asset this service needs.
    pub fn binary_name(self) -> &'static str {
        INDEXER_ASSET
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
        Self::with_options(LogMode::Pipe)
    }

    /// A manager whose processes log to files and outlive it.
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

    /// Stop a network started by an earlier command: ask it to stop, then
    /// end the recorded process if it is still running. Also ends a
    /// `qubitcoind` left by a Labcoat release that used one, since it would
    /// hold the network's port.
    pub fn stop_running(config: &IsomerConfig) {
        let pid_file = get_network_dir().join(PID_FILE);
        let asked = crate::rpc::call_blocking_with_timeout(
            config,
            "stop",
            serde_json::json!([]),
            Duration::from_secs(2),
        )
        .is_ok();
        // The node deletes its pid file as it exits. Its process can linger
        // as a zombie until something reaps it, so the file, not the process
        // table, says when it has stopped.
        let deadline = Instant::now() + Duration::from_secs(if asked { 5 } else { 0 });
        while pid_file.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        if let Some(pid) = recorded_pid() {
            if process_alive(pid) {
                signal(pid, true);
            }
        }
        let _ = std::fs::remove_file(&pid_file);

        #[cfg(unix)]
        {
            let _ = Command::new("pkill").args(["-x", "qubitcoind"]).output();
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

    /// The arguments `labcoat node` is started with.
    fn build_args(config: &IsomerConfig) -> Vec<String> {
        vec![
            "node".into(),
            "--port".into(),
            config.ports.rpc.to_string(),
            "--data-dir".into(),
            get_network_dir().display().to_string(),
            "--indexer".into(),
            get_bin_dir().join(INDEXER_ASSET).display().to_string(),
            "--cache-dir".into(),
            get_compiled_dir().display().to_string(),
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
        let indexer = get_bin_dir().join(INDEXER_ASSET);
        if !indexer.exists() {
            return Err(format!(
                "Runtime asset not found: {}. Run `labcoat up` without --no-download.",
                indexer.display()
            ));
        }
        std::fs::create_dir_all(get_network_dir())
            .map_err(|e| format!("Failed to create the network data directory: {e}"))?;
        std::fs::create_dir_all(get_logs_dir())
            .map_err(|e| format!("Failed to create log directory: {e}"))?;

        let executable = std::env::current_exe()
            .map_err(|e| format!("Cannot find the labcoat executable to run the network: {e}"))?;
        let mut command = Command::new(executable);
        command
            .args(Self::build_args(config))
            .env("RUST_LOG", "info");

        match self.log_mode {
            LogMode::Pipe => {
                command.stdout(Stdio::piped()).stderr(Stdio::piped());
            }
            LogMode::File => {
                let log_path = get_logs_dir().join(format!("{}.log", service.id()));
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
        #[cfg(unix)]
        {
            // Its own process group, so Ctrl-C in the terminal that ran
            // `labcoat up` does not stop the network.
            use std::os::unix::process::CommandExt;
            command.process_group(0);
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

    /// Whether the process this manager started has exited, and how.
    pub fn exit_status(&mut self, service: ServiceId) -> Option<String> {
        let info = self.processes.get_mut(&service)?;
        match info.child.try_wait() {
            Ok(Some(status)) => Some(status.to_string()),
            _ => None,
        }
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
                        service: ServiceId::Node.id().into(),
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
        signal(info.child.id(), false);
        let deadline = Instant::now() + Duration::from_secs(2);
        while info.child.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        if info.child.try_wait().ok().flatten().is_none() {
            let _ = info.child.kill();
        }
        info.child
            .wait()
            .map(|_| ())
            .map_err(|e| format!("Error waiting for {}: {e}", service.display_name()))
    }

    pub fn start_all(&mut self, config: &IsomerConfig) -> Result<(), String> {
        self.start_service(ServiceId::Node, config)
    }

    pub fn stop_all(&mut self) -> Result<(), String> {
        self.stop_service(ServiceId::Node)
    }

    /// Stop the network and delete its chain.
    pub fn reset_data(&mut self, config: &IsomerConfig) -> Result<(), String> {
        let _ = self.stop_all();
        Self::stop_running(config);
        let dir = get_network_dir();
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
        config.ports.rpc
    }

    pub async fn check_health(&self, service: ServiceId, config: &IsomerConfig) -> bool {
        self.processes.contains_key(&service) && Self::probe_health(service, config).await
    }

    pub async fn probe_health(_service: ServiceId, config: &IsomerConfig) -> bool {
        crate::rpc::is_own_node(config).await
    }
}

impl Drop for ProcessManager {
    fn drop(&mut self) {
        let _ = self.stop_all();
    }
}

fn recorded_pid() -> Option<u32> {
    std::fs::read_to_string(get_network_dir().join(PID_FILE))
        .ok()?
        .trim()
        .parse()
        .ok()
        .filter(|pid| *pid != std::process::id())
}

fn process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // SAFETY: signal 0 only checks that the process exists.
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

/// Ask a process to stop, or with `force`, end it.
fn signal(pid: u32, force: bool) {
    #[cfg(unix)]
    {
        let signal = if force { libc::SIGKILL } else { libc::SIGTERM };
        // SAFETY: signals a process this crate started or recorded.
        unsafe {
            libc::kill(pid as i32, signal);
        }
    }
    #[cfg(windows)]
    {
        let _ = force;
        let _ = Command::new("taskkill")
            .args(["/F", "/PID", &pid.to_string()])
            .output();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_network_runs_the_labcoat_node_command() {
        let args = ProcessManager::build_args(&IsomerConfig::default());
        assert_eq!(args[0], "node");
        assert_eq!(args[1..3], ["--port".to_string(), "18443".to_string()]);
        assert!(args.iter().any(|arg| arg.ends_with(INDEXER_ASSET)));
        assert!(args.contains(&"--data-dir".to_string()));
        assert!(args.contains(&"--cache-dir".to_string()));
    }

    #[test]
    fn runtime_has_one_service() {
        assert_eq!(ServiceId::all(), vec![ServiceId::Node]);
        assert_eq!(ServiceId::Node.id(), "node");
        assert_eq!(ServiceId::Node.binary_name(), "alkanes.wasm");
    }

    #[test]
    fn this_process_is_never_its_own_orphan() {
        assert!(process_alive(std::process::id()));
    }
}
