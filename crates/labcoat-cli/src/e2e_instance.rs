//! The network `labcoat test --e2e` runs against.
//!
//! Each project gets one of its own, apart from the Labcoat Network the
//! developer works on: its own chain, port, wallet, and deployment records,
//! all under the project's `.labcoat/e2e/`. A run can reset its chain and
//! deploy onto it without touching the shared chain or the records of what
//! is deployed there.
//!
//! The instance is a set of environment variables, because that is what
//! reaches every `labcoat` command the tests go on to run:
//!
//! ```text
//! LABCOAT_DATA_HOME    the instance's chain, logs, and built-in wallet
//! LABCOAT_RUNTIME_DIR  the runtime already installed for the developer
//! LABCOAT_RPC_PORT     a port that was free when the run started
//! LABCOAT_RPC_URL      the same port, over any `rpc_url` in labcoat.toml
//! LABCOAT_RECORDS_DIR  labcoat.lock, deployments/, and durable state
//! ```

use crate::contract::EnvelopeError;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Where a project keeps its e2e network, relative to its root.
const INSTANCE_DIR: &str = ".labcoat/e2e";

#[derive(Debug)]
pub struct Instance {
    pub data_home: PathBuf,
    pub records_dir: PathBuf,
    pub rpc_url: String,
    /// Held for the whole run: two runs must not share one chain.
    _lock: std::fs::File,
}

static CURRENT: OnceLock<Instance> = OnceLock::new();

/// The instance this process entered, if it is an e2e run.
pub fn current() -> Option<&'static Instance> {
    CURRENT.get()
}

pub fn data_home(project_root: &Path) -> PathBuf {
    project_root.join(INSTANCE_DIR).join("network")
}

pub fn records_dir(project_root: &Path) -> PathBuf {
    project_root.join(INSTANCE_DIR).join("records")
}

fn setup_error(message: String) -> EnvelopeError {
    EnvelopeError {
        code: "TOOLKIT_ERROR",
        message,
        hint: "check permissions on the project's .labcoat directory",
    }
}

/// Point this process at the project's e2e network. Changes the process
/// environment, so it must run before any other thread starts.
pub fn enter(project_root: &Path) -> Result<(), EnvelopeError> {
    // Resolved before the data home moves: the instance runs the runtime
    // the developer already has instead of downloading a second copy.
    let runtime_dir = isomer_core::config::get_bin_dir();

    let data_home = data_home(project_root);
    let records_dir = records_dir(project_root);
    for dir in [&data_home, &records_dir] {
        std::fs::create_dir_all(dir)
            .map_err(|e| setup_error(format!("cannot create {}: {e}", dir.display())))?;
    }
    let lock_path = data_home.join("run.lock");
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|e| setup_error(format!("cannot open {}: {e}", lock_path.display())))?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(std::fs::TryLockError::WouldBlock) => {
            return Err(EnvelopeError {
                code: "TEST_FAILED",
                message: "another `labcoat test --e2e` run is using this project's e2e network"
                    .into(),
                hint: "wait for the other run to finish, then retry",
            })
        }
        Err(std::fs::TryLockError::Error(e)) => {
            return Err(setup_error(format!(
                "cannot lock {}: {e}",
                lock_path.display()
            )))
        }
    }
    let port = free_port().map_err(|e| setup_error(format!("no free local port: {e}")))?;
    let rpc_url = format!("http://127.0.0.1:{port}");

    std::env::set_var("LABCOAT_RUNTIME_DIR", &runtime_dir);
    std::env::set_var("LABCOAT_DATA_HOME", &data_home);
    std::env::set_var("LABCOAT_RPC_PORT", port.to_string());
    std::env::set_var("LABCOAT_RPC_URL", &rpc_url);
    std::env::set_var(labcoat_core::records::RECORDS_DIR_ENV, &records_dir);

    let _ = CURRENT.set(Instance {
        data_home,
        records_dir,
        rpc_url,
        _lock: lock,
    });
    Ok(())
}

/// A port nothing is listening on. Another process could take it before the
/// network binds it; the network then fails to start and the run says so.
fn free_port() -> std::io::Result<u16> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    Ok(listener.local_addr()?.port())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_instance_lives_under_the_projects_ignored_directory() {
        let root = Path::new("/project");
        assert_eq!(data_home(root), Path::new("/project/.labcoat/e2e/network"));
        assert_eq!(
            records_dir(root),
            Path::new("/project/.labcoat/e2e/records")
        );
    }

    #[test]
    fn free_ports_can_be_bound() {
        let port = free_port().unwrap();
        assert_ne!(port, 0);
        assert!(std::net::TcpListener::bind(("127.0.0.1", port)).is_ok());
    }
}
