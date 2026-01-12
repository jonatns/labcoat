mod binary_manager;
mod commands;
mod config;
mod process_manager;
mod state;

use std::sync::Arc;
use tauri::Manager;
use tokio::sync::RwLock;

pub use state::AppState;

/// Initialize and run the Isomer desktop application
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("isomer=debug".parse().unwrap()),
        )
        .init();

    tracing::info!("Starting Isomer - Alkanes Development Environment");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // Initialize application state
            let state = Arc::new(RwLock::new(AppState::new(app.handle().clone())));
            app.manage(state);

            tracing::info!("Isomer initialized successfully");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::start_services,
            commands::stop_services,
            commands::reset_chain,
            commands::get_logs,
            commands::clear_logs,
            commands::faucet,
            commands::mine_blocks,
            commands::get_accounts,
            commands::check_binaries,
            commands::download_binaries,
            commands::download_wasm,
            commands::get_config,
            commands::update_config,
            commands::check_service_health,
            commands::get_extension_path,
            commands::get_espo_blocks,
            commands::get_latest_block,
            commands::get_block_details,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
