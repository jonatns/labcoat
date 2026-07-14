//! # isomer-core
//!
//! Headless Alkanes devnet engine: binary management, service
//! orchestration, chain control, and devnet queries. Every frontend —
//! the Isomer desktop app (Tauri), the `labcoat` CLI, agents — drives
//! the devnet through this one crate so behavior stays identical
//! across surfaces.

pub mod alkanes_wallets;
pub mod binary_manager;
pub mod config;
pub mod devnet;
pub mod espo;
pub mod process_manager;
pub mod rpc;
pub mod state;

pub use binary_manager::{BinaryInfo, BinaryManager, BinaryStatus};
pub use config::{get_bin_dir, get_data_dir, get_logs_dir, get_runtime_dir, IsomerConfig};
pub use devnet::Devnet;
pub use process_manager::{LogEntry, ProcessManager, ServiceId};
pub use state::{Account, AddressInfo, AlkanesWallet, ServiceInfo, ServiceStatus, SystemStatus};
