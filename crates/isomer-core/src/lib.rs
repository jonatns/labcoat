//! # isomer-core
//!
//! Headless Labcoat Network engine: binary management, service
//! orchestration, chain control, and network queries. The `labcoat` CLI
//! and agents drive Labcoat Network through this crate.

pub mod binary_manager;
pub mod config;
pub mod faucet;
pub mod labcoat_network;
pub mod process_manager;
pub mod rpc;
pub mod state;

pub use binary_manager::{BinaryInfo, BinaryManager, BinaryStatus};
pub use config::{
    get_bin_dir, get_data_dir, get_logs_dir, get_qubitcoin_dir, get_runtime_dir, IsomerConfig,
};
pub use labcoat_network::LabcoatNetwork;
pub use process_manager::{LogEntry, ProcessManager, ServiceId};
pub use state::{ServiceInfo, ServiceStatus, SystemStatus};
