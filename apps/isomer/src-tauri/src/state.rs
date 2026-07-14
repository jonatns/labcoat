//! Application state management
//!
//! The Tauri-side wrapper over the isomer-core devnet engine. All
//! domain types live in isomer-core; this only adds the app handle.

use isomer_core::{IsomerConfig, ProcessManager, SystemStatus};
use serde::Serialize;
use tauri::Emitter;

/// Main application state
pub struct AppState {
    pub config: IsomerConfig,
    pub process_manager: ProcessManager,
    pub accounts: Vec<isomer_core::Account>,
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
        let services = self.process_manager.get_all_status(&self.config);
        let is_ready = services.iter().all(|s| s.status == "running");

        SystemStatus {
            services,
            block_height: self.block_height,
            mempool_size: self.mempool_size,
            is_ready,
        }
    }

    /// Emit an event to the frontend
    #[allow(dead_code)]
    pub fn emit<S: Serialize + Clone>(&self, event: &str, payload: S) {
        if let Err(e) = self.app_handle.emit(event, payload) {
            tracing::error!("Failed to emit event {}: {}", event, e);
        }
    }
}
