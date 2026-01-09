//! Tauri command handlers
//!
//! These are the functions exposed to the frontend via Tauri's invoke system

use crate::binary_manager::{BinaryInfo, BinaryManager};
use crate::config::IsomerConfig;
use crate::state::{Account, AppState, SystemStatus};
use std::sync::Arc;
use tauri::{Emitter, State};
use tokio::sync::RwLock;

type SharedState = Arc<RwLock<AppState>>;

/// Get the current system status
#[tauri::command]
pub async fn get_status(state: State<'_, SharedState>) -> Result<SystemStatus, String> {
    let state = state.read().await;
    Ok(state.get_status())
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

/// Mine a specified number of blocks
#[tauri::command]
pub async fn mine_blocks(
    count: u32,
    address: Option<String>,
    state: State<'_, SharedState>,
) -> Result<u64, String> {
    let state = state.read().await;
    let config = &state.config;

    // Use first account address if none specified
    let mine_to = address.unwrap_or_else(|| {
        state
            .accounts
            .first()
            .map(|a| a.address.clone())
            .unwrap_or_else(|| "bcrt1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqdku202".to_string())
    });

    // Call bitcoin-cli to mine blocks
    let rpc_url = format!("http://127.0.0.1:{}", config.ports.bitcoind_rpc);
    let auth = format!("{}:{}", config.bitcoind.rpc_user, config.bitcoind.rpc_password);

    let client = reqwest::Client::new();
    let response = client
        .post(&rpc_url)
        .basic_auth(&config.bitcoind.rpc_user, Some(&config.bitcoind.rpc_password))
        .json(&serde_json::json!({
            "jsonrpc": "1.0",
            "id": "isomer",
            "method": "generatetoaddress",
            "params": [count, mine_to]
        }))
        .send()
        .await
        .map_err(|e| format!("RPC call failed: {}", e))?;

    let result: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    if let Some(error) = result.get("error").and_then(|e| e.as_object()) {
        return Err(format!(
            "Bitcoin RPC error: {}",
            error.get("message").and_then(|m| m.as_str()).unwrap_or("unknown")
        ));
    }

    // Get new block height
    let height_response = client
        .post(&rpc_url)
        .basic_auth(&config.bitcoind.rpc_user, Some(&config.bitcoind.rpc_password))
        .json(&serde_json::json!({
            "jsonrpc": "1.0",
            "id": "isomer",
            "method": "getblockcount",
            "params": []
        }))
        .send()
        .await
        .map_err(|e| format!("RPC call failed: {}", e))?;

    let height_result: serde_json::Value = height_response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    let new_height = height_result
        .get("result")
        .and_then(|r| r.as_u64())
        .unwrap_or(0);

    Ok(new_height)
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
    let manager = BinaryManager::new();

    manager
        .download_all(move |service, progress| {
            let _ = app.emit("download-progress", serde_json::json!({
                "service": service.display_name(),
                "progress": progress
            }));
        })
        .await
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
    config.save().map_err(|e| format!("Failed to save config: {}", e))?;
    state.config = config;
    Ok(())
}
