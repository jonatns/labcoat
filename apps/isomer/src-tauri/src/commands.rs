//! Tauri command handlers
//!
//! Thin glue between the frontend's `invoke()` calls and isomer-core.
//! Command names, signatures, and JSON shapes are unchanged from the
//! pre-monorepo app — all logic now lives in the shared core crate.

use crate::state::AppState;
use isomer_core::espo::{EspoAlkanesResponse, EspoCarouselResponse};
use isomer_core::rpc::{BlockDetails, BlockSummary};
use isomer_core::{
    Account, AlkanesWallet, BinaryInfo, BinaryManager, IsomerConfig, LogEntry, ServiceId,
    SystemStatus,
};
use std::sync::Arc;
use tauri::{Emitter, State};
use tokio::sync::RwLock;

type SharedState = Arc<RwLock<AppState>>;

/// Get the current system status
#[tauri::command]
pub async fn get_status(state: State<'_, SharedState>) -> Result<SystemStatus, String> {
    // 1. Get process status from state
    let mut state_guard = state.write().await;
    let mut system_status = state_guard.get_status();
    let config = state_guard.config.clone();
    drop(state_guard); // Release lock

    // 2. Fetch live info from Bitcoind if running
    let bitcoind_running = system_status
        .services
        .iter()
        .any(|s| s.id == "bitcoind" && s.status == "running");

    if bitcoind_running {
        if let Some(height) = isomer_core::rpc::try_block_count(&config).await {
            system_status.block_height = height;
        }
        if let Some(size) = isomer_core::rpc::try_mempool_size(&config).await {
            system_status.mempool_size = size;
        }
    }

    Ok(system_status)
}

/// Start all services
#[tauri::command]
pub async fn start_services(state: State<'_, SharedState>) -> Result<(), String> {
    let mut state = state.write().await;
    let config = state.config.clone();
    state.process_manager.start_all(&config)
}

/// Stop all services
#[tauri::command]
pub async fn stop_services(state: State<'_, SharedState>) -> Result<(), String> {
    let mut state = state.write().await;
    state.process_manager.stop_all()
}

/// Reset chain - stops services and clears all data
#[tauri::command]
pub async fn reset_chain(state: State<'_, SharedState>) -> Result<(), String> {
    let mut state = state.write().await;
    state.process_manager.reset_data()
}

/// Get service logs
#[tauri::command]
pub async fn get_logs(
    service: Option<String>,
    limit: Option<usize>,
    state: State<'_, SharedState>,
) -> Result<Vec<LogEntry>, String> {
    let state = state.read().await;
    Ok(state
        .process_manager
        .get_logs(service, limit.unwrap_or(500)))
}

/// Clear all logs
#[tauri::command]
pub async fn clear_logs(state: State<'_, SharedState>) -> Result<(), String> {
    let state = state.read().await;
    state.process_manager.clear_logs();
    Ok(())
}

/// Faucet - send BTC from dev wallet to any address
#[tauri::command]
pub async fn faucet(
    address: String,
    amount: f64,
    state: State<'_, SharedState>,
) -> Result<String, String> {
    let config = state.read().await.config.clone();
    isomer_core::rpc::faucet(&config, &address, amount).await
}

/// Mine a specified number of blocks
#[tauri::command]
pub async fn mine_blocks(
    count: u32,
    address: Option<String>,
    state: State<'_, SharedState>,
) -> Result<u64, String> {
    let state = state.read().await;
    let config = state.config.clone();

    // Use first account address if none specified
    let mine_to = address.unwrap_or_else(|| {
        state
            .accounts
            .first()
            .map(|a| a.address.clone())
            .unwrap_or_else(|| isomer_core::rpc::DEFAULT_MINE_ADDRESS.to_string())
    });
    drop(state);

    isomer_core::rpc::mine_blocks(&config, count, &mine_to).await
}

/// Get all pre-funded accounts
#[tauri::command]
pub async fn get_accounts(state: State<'_, SharedState>) -> Result<Vec<Account>, String> {
    let state = state.read().await;
    Ok(state.accounts.clone())
}

/// Check status of all binaries
#[tauri::command]
pub async fn check_binaries() -> Result<Vec<BinaryInfo>, String> {
    let manager = BinaryManager::new();
    Ok(manager.check_all())
}

/// Download missing binaries
#[tauri::command]
pub async fn download_binaries(app: tauri::AppHandle) -> Result<(), String> {
    let mut manager = BinaryManager::new();

    // First download alkanes.wasm for metashrew
    BinaryManager::download_alkanes_wasm().await?;

    // Then download all service binaries
    manager
        .download_all(move |service, progress| {
            let _ = app.emit(
                "download-progress",
                serde_json::json!({
                    "service": service.display_name(),
                    "progress": progress
                }),
            );
        })
        .await
}

/// Download just the alkanes.wasm file
#[tauri::command]
pub async fn download_wasm() -> Result<(), String> {
    BinaryManager::download_alkanes_wasm().await
}

/// Get current configuration
#[tauri::command]
pub async fn get_config(state: State<'_, SharedState>) -> Result<IsomerConfig, String> {
    let state = state.read().await;
    Ok(state.config.clone())
}

/// Update configuration
#[tauri::command]
pub async fn update_config(
    config: IsomerConfig,
    state: State<'_, SharedState>,
) -> Result<(), String> {
    let mut state = state.write().await;
    config
        .save()
        .map_err(|e| format!("Failed to save config: {}", e))?;
    state.config = config;
    Ok(())
}

/// Check health of a specific service
#[tauri::command]
pub async fn check_service_health(
    service: ServiceId,
    state: State<'_, SharedState>,
) -> Result<bool, String> {
    let state = state.read().await;
    Ok(state
        .process_manager
        .check_health(service, &state.config)
        .await)
}

/// Get the extension path, downloading if necessary
/// Returns the path to the extension directory for manual loading in Chrome
#[tauri::command]
pub async fn get_extension_path() -> Result<String, String> {
    let extension_dir = BinaryManager::download_extension().await?;
    Ok(extension_dir.display().to_string())
}

/// Check if the extension is installed
#[tauri::command]
pub async fn check_extension_status() -> Result<bool, String> {
    Ok(BinaryManager::is_extension_installed())
}

// ─────────────────────────────────────────────────────────────────────────────
// Espo Explorer API
// ─────────────────────────────────────────────────────────────────────────────

/// Fetch all deployed alkanes from Espo API
#[tauri::command]
pub async fn get_all_alkanes(
    page: Option<u64>,
    limit: Option<u64>,
    state: State<'_, SharedState>,
) -> Result<EspoAlkanesResponse, String> {
    let config = state.read().await.config.clone();
    isomer_core::espo::get_all_alkanes(&config, page, limit).await
}

/// Fetch carousel blocks from Espo explorer API
#[tauri::command]
pub async fn get_espo_blocks(
    center: Option<u64>,
    radius: Option<u64>,
    state: State<'_, SharedState>,
) -> Result<EspoCarouselResponse, String> {
    let config = state.read().await.config.clone();
    isomer_core::espo::get_espo_blocks(&config, center, radius).await
}

/// Get the latest block info directly from Bitcoin Core (for optimistic UI updates)
#[tauri::command]
pub async fn get_latest_block(state: State<'_, SharedState>) -> Result<BlockSummary, String> {
    let config = state.read().await.config.clone();
    isomer_core::rpc::latest_block(&config).await
}

/// Get full block details including transactions from Bitcoin Core + Alkanes Trace info
#[tauri::command]
pub async fn get_block_details(
    height: u64,
    state: State<'_, SharedState>,
) -> Result<BlockDetails, String> {
    let config = state.read().await.config.clone();
    isomer_core::rpc::block_details(&config, height).await
}

// ─────────────────────────────────────────────────────────────────────────────
// Alkanes Wallet API
// ─────────────────────────────────────────────────────────────────────────────

/// List all alkanes-cli wallets in ~/.alkanes/
#[tauri::command]
pub async fn get_alkanes_wallets() -> Result<Vec<AlkanesWallet>, String> {
    isomer_core::alkanes_wallets::list_wallets()
}

/// Get details for a specific wallet via alkanes-cli
#[tauri::command]
pub async fn get_alkane_wallet_details(
    wallet_path: String,
    state: State<'_, SharedState>,
) -> Result<AlkanesWallet, String> {
    // Sync only when bitcoind is running
    let bitcoind_running = {
        let mut state_guard = state.write().await;
        let status = state_guard.get_status();
        status
            .services
            .iter()
            .any(|s| s.id == "bitcoind" && s.status == "running")
    };

    // alkanes-cli invocations are blocking subprocess calls
    tokio::task::spawn_blocking(move || {
        isomer_core::alkanes_wallets::wallet_details(&wallet_path, bitcoind_running)
    })
    .await
    .map_err(|e| format!("wallet task failed: {}", e))?
}

/// Fund an alkanes-cli wallet from the dev wallet
#[tauri::command]
pub async fn fund_alkane_wallet(
    address: String,
    amount: f64,
    state: State<'_, SharedState>,
) -> Result<String, String> {
    // 1. Send funds
    let txid = faucet(address.clone(), amount, state.clone()).await?;

    // 2. Mine a block to confirm (regtest: immediate is fine)
    match mine_blocks(1, None, state).await {
        Ok(_) => Ok(txid),
        Err(e) => Err(format!(
            "Funds sent (txid: {}) but mining failed: {}",
            txid, e
        )),
    }
}
