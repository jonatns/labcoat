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

/// Get the extension path, downloading if necessary
/// Returns the path to the extension directory for manual loading in Chrome
#[tauri::command]
pub async fn get_extension_path() -> Result<String, String> {
    use crate::binary_manager::BinaryManager;
    
    let extension_dir = BinaryManager::download_extension().await?;
    Ok(extension_dir.display().to_string())
}

/// Check if the extension is installed
#[tauri::command]
pub async fn check_extension_status() -> Result<bool, String> {
    use crate::binary_manager::BinaryManager;
    Ok(BinaryManager::is_extension_installed())
}

// ─────────────────────────────────────────────────────────────────────────────
// Espo Explorer API
// ─────────────────────────────────────────────────────────────────────────────

#[derive(serde::Serialize, Clone)]
pub struct EspoCarouselBlock {
    pub height: u64,
    pub traces: u64,
    pub time: Option<u64>,
}

#[derive(serde::Serialize)]
pub struct BlockDetails {
    pub height: u64,
    pub hash: String,
    pub time: Option<u64>,
    pub transactions: Vec<String>,
}

#[derive(serde::Serialize)]
pub struct EspoCarouselResponse {
    pub espo_tip: u64,
    pub blocks: Vec<EspoCarouselBlock>,
}

/// Fetch carousel blocks from Espo explorer API
#[tauri::command]
pub async fn get_espo_blocks(
    center: Option<u64>,
    radius: Option<u64>,
) -> Result<EspoCarouselResponse, String> {
    let radius = radius.unwrap_or(10);
    let mut url = format!("http://localhost:8081/api/blocks/carousel?radius={}", radius);
    
    if let Some(c) = center {
        url.push_str(&format!("&center={}", c));
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Failed to connect to Espo: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Espo API error: {}", response.status()));
    }

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse Espo response: {}", e))?;

    let espo_tip = json
        .get("espo_tip")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let blocks = json
        .get("blocks")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|b| {
                    Some(EspoCarouselBlock {
                        height: b.get("height")?.as_u64()?,
                        traces: b.get("traces")?.as_u64().unwrap_or(0),
                        time: b.get("time").and_then(|t| t.as_u64()),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(EspoCarouselResponse { espo_tip, blocks })
}

/// Get the latest block info directly from Bitcoin Core (for optimistic UI updates)
#[tauri::command]
pub async fn get_latest_block(state: State<'_, SharedState>) -> Result<EspoCarouselBlock, String> {
    let state = state.read().await;
    let config = &state.config;

    let rpc_url = format!("http://127.0.0.1:{}", config.ports.bitcoind_rpc);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(500))
        .build()
        .map_err(|e| format!("{}", e))?;

    // 1. Get block count
    let count_res = client
        .post(&rpc_url)
        .basic_auth(&config.bitcoind.rpc_user, Some(&config.bitcoind.rpc_password))
        .json(&serde_json::json!({
            "jsonrpc": "1.0",
            "id": "isomer-bestblock",
            "method": "getblockcount",
            "params": []
        }))
        .send()
        .await
        .map_err(|e| format!("Failed to connect to Bitcoin Core: {}", e))?
        .json::<serde_json::Value>()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    let height = count_res
        .get("result")
        .and_then(|v| v.as_u64())
        .ok_or("Invalid block count response")?;

    // 2. Get block hash
    let hash_res = client
        .post(&rpc_url)
        .basic_auth(&config.bitcoind.rpc_user, Some(&config.bitcoind.rpc_password))
        .json(&serde_json::json!({
            "jsonrpc": "1.0",
            "id": "isomer-bestblockhash",
            "method": "getblockhash",
            "params": [height]
        }))
        .send()
        .await
        .map_err(|e| format!("Failed to get block hash: {}", e))?
        .json::<serde_json::Value>()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    let hash = hash_res
        .get("result")
        .and_then(|v| v.as_str())
        .ok_or("Invalid block hash response")?;

    // 3. Get block details (time)
    let block_res = client
        .post(&rpc_url)
        .basic_auth(&config.bitcoind.rpc_user, Some(&config.bitcoind.rpc_password))
        .json(&serde_json::json!({
            "jsonrpc": "1.0",
            "id": "isomer-block",
            "method": "getblock",
            "params": [hash]
        }))
        .send()
        .await
        .map_err(|e| format!("Failed to get block details: {}", e))?
        .json::<serde_json::Value>()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    let time = block_res
        .get("result")
        .and_then(|r| r.get("time"))
        .and_then(|t| t.as_u64());

    Ok(EspoCarouselBlock {
        height,
        traces: 0, // Bitcoin Core doesn't know about traces, Espo will fill this later
        time,
    })
}

/// Get full block details including transactions from Bitcoin Core
#[tauri::command]
pub async fn get_block_details(height: u64, state: State<'_, SharedState>) -> Result<BlockDetails, String> {
    let state = state.read().await;
    let config = &state.config;

    let rpc_url = format!("http://127.0.0.1:{}", config.ports.bitcoind_rpc);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(1000))
        .build()
        .map_err(|e| format!("{}", e))?;

    // 1. Get block hash
    let hash_res = client
        .post(&rpc_url)
        .basic_auth(&config.bitcoind.rpc_user, Some(&config.bitcoind.rpc_password))
        .json(&serde_json::json!({
            "jsonrpc": "1.0",
            "id": "isomer-blockdetails",
            "method": "getblockhash",
            "params": [height]
        }))
        .send()
        .await
        .map_err(|e| format!("Failed to get block hash: {}", e))?
        .json::<serde_json::Value>()
        .await
        .map_err(|e| format!("Failed to parse hash response: {}", e))?;

    let hash = hash_res
        .get("result")
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("Block not found at height {}", height))?;

    // 2. Get block details
    let block_res = client
        .post(&rpc_url)
        .basic_auth(&config.bitcoind.rpc_user, Some(&config.bitcoind.rpc_password))
        .json(&serde_json::json!({
            "jsonrpc": "1.0",
            "id": "isomer-blockdetails",
            "method": "getblock",
            "params": [hash, 1] // 1 for JSON with txids
        }))
        .send()
        .await
        .map_err(|e| format!("Failed to get block details: {}", e))?
        .json::<serde_json::Value>()
        .await
        .map_err(|e| format!("Failed to parse block response: {}", e))?;

    let result = block_res.get("result").ok_or("No result in block details")?;
    let time = result.get("time").and_then(|t| t.as_u64());
    let transactions = result.get("tx")
        .and_then(|t| t.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();

    Ok(BlockDetails {
        height,
        hash: hash.to_string(),
        time,
        transactions,
    })
}
