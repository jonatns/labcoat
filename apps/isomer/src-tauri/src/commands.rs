//! Tauri command handlers
//!
//! These are the functions exposed to the frontend via Tauri's invoke system

use crate::binary_manager::{BinaryInfo, BinaryManager};
use crate::config::IsomerConfig;
use crate::process_manager::ServiceId;
use crate::state::{Account, AppState, ServiceStatus, SystemStatus};
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
    // Check if bitcoind is running first
    let bitcoind_running = system_status
        .services
        .iter()
        .any(|s| s.id == "bitcoind" && s.status == "running");

    if bitcoind_running {
        let rpc_url = format!("http://127.0.0.1:{}", config.ports.bitcoind_rpc);
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(500))
            .build()
            .unwrap_or_default();

        // Get Block Count
        let count_req = client
            .post(&rpc_url)
            .basic_auth(
                &config.bitcoind.rpc_user,
                Some(&config.bitcoind.rpc_password),
            )
            .json(&serde_json::json!({
                "jsonrpc": "1.0",
                "id": "isomer-status",
                "method": "getblockcount",
                "params": []
            }));

        if let Ok(res) = count_req.send().await {
            if let Ok(json) = res.json::<serde_json::Value>().await {
                if let Some(height) = json.get("result").and_then(|h| h.as_u64()) {
                    system_status.block_height = height;
                }
            }
        }

        // Get Mempool Info
        let mempool_req = client
            .post(&rpc_url)
            .basic_auth(
                &config.bitcoind.rpc_user,
                Some(&config.bitcoind.rpc_password),
            )
            .json(&serde_json::json!({
                "jsonrpc": "1.0",
                "id": "isomer-status",
                "method": "getmempoolinfo",
                "params": []
            }));

        if let Ok(res) = mempool_req.send().await {
            if let Ok(json) = res.json::<serde_json::Value>().await {
                if let Some(size) = json
                    .get("result")
                    .and_then(|r| r.get("size"))
                    .and_then(|s| s.as_u64())
                {
                    system_status.mempool_size = size as usize;
                }
            }
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
) -> Result<Vec<crate::process_manager::LogEntry>, String> {
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
    let state = state.read().await;
    let config = &state.config;

    // Default to 1 BTC if not specified or 0
    let amount_btc = if amount <= 0.0 { 1.0 } else { amount };

    let rpc_url = format!("http://127.0.0.1:{}", config.ports.bitcoind_rpc);
    let wallet_rpc_url = format!("{}/wallet/dev", rpc_url);

    let client = reqwest::Client::new();

    // Send from dev wallet to the target address
    let response = client
        .post(&wallet_rpc_url)
        .basic_auth(
            &config.bitcoind.rpc_user,
            Some(&config.bitcoind.rpc_password),
        )
        .json(&serde_json::json!({
            "jsonrpc": "1.0",
            "id": "isomer",
            "method": "sendtoaddress",
            "params": [address, amount_btc]
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
            "Faucet error: {}",
            error
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown")
        ));
    }

    let txid = result
        .get("result")
        .and_then(|r| r.as_str())
        .unwrap_or("unknown")
        .to_string();

    tracing::info!(
        "Faucet: sent {} BTC to {} (txid: {})",
        amount_btc,
        address,
        txid
    );

    Ok(txid)
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

    if count > 1000 {
        return Err("Cannot mine more than 1000 blocks at once.".to_string());
    }

    // Use first account address if none specified
    let mine_to = address.unwrap_or_else(|| {
        state
            .accounts
            .first()
            .map(|a| a.address.clone())
            .unwrap_or_else(|| "bcrt1q9zuctyd46l7sdedccdk47335lzsmjz2wngdv3u".to_string())
    });

    // Call bitcoin-cli to mine blocks
    let rpc_url = format!("http://127.0.0.1:{}", config.ports.bitcoind_rpc);

    let client = reqwest::Client::new();
    let response = client
        .post(&rpc_url)
        .basic_auth(
            &config.bitcoind.rpc_user,
            Some(&config.bitcoind.rpc_password),
        )
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
            error
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown")
        ));
    }

    // Get new block height
    let height_response = client
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
