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
pub struct EspoCarouselResponse {
    pub espo_tip: u64,
    pub blocks: Vec<EspoCarouselBlock>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct AlkaneInfo {
    pub alkane: String,
    pub creation_txid: String,
    pub creation_height: u64,
    pub creation_timestamp: Option<u64>,
    pub name: Option<String>,
    pub symbol: Option<String>,
    pub holder_count: u64,
}

#[derive(serde::Serialize)]
pub struct EspoAlkanesResponse {
    pub ok: bool,
    pub page: u64,
    pub limit: u64,
    pub total: u64,
    pub items: Vec<AlkaneInfo>,
}

/// Fetch all deployed alkanes from Espo API
#[tauri::command]
pub async fn get_all_alkanes(
    page: Option<u64>,
    limit: Option<u64>,
) -> Result<EspoAlkanesResponse, String> {
    let page = page.unwrap_or(1);
    let limit = limit.unwrap_or(50);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": "isomer",
        "method": "essentials.get_all_alkanes",
        "params": {
            "page": page,
            "limit": limit
        }
    });

    let response = client
        .post("http://127.0.0.1:8083/rpc")
        .header("Content-Type", "application/json")
        .body(payload.to_string())
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

    // The RPC returns { "jsonrpc": "2.0", "id": "isomer", "result": { ... } }
    let result = json.get("result").ok_or("Missing result in response")?;

    let ok = result.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
    let page = result.get("page").and_then(|v| v.as_u64()).unwrap_or(1);
    let limit = result.get("limit").and_then(|v| v.as_u64()).unwrap_or(50);
    let total = result.get("total").and_then(|v| v.as_u64()).unwrap_or(0);

    let items: Vec<AlkaneInfo> = result
        .get("items")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    Some(AlkaneInfo {
                        alkane: item.get("alkane")?.as_str()?.to_string(),
                        creation_txid: item.get("creation_txid")?.as_str()?.to_string(),
                        creation_height: item.get("creation_height")?.as_u64()?,
                        creation_timestamp: item.get("creation_timestamp").and_then(|v| v.as_u64()),
                        name: item.get("name").and_then(|v| v.as_str()).map(|s| s.to_string()),
                        symbol: item.get("symbol").and_then(|v| v.as_str()).map(|s| s.to_string()),
                        holder_count: item.get("holder_count").and_then(|v| v.as_u64()).unwrap_or(0),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(EspoAlkanesResponse {
        ok,
        page,
        limit,
        total,
        items,
    })
}

/// Fetch carousel blocks from Espo explorer API
#[tauri::command]
pub async fn get_espo_blocks(
    center: Option<u64>,
    radius: Option<u64>,
) -> Result<EspoCarouselResponse, String> {
    let radius = radius.unwrap_or(10);
    let mut url = format!("http://127.0.0.1:8081/api/blocks/carousel?radius={}", radius);
    
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

#[derive(serde::Serialize)]
pub struct TransactionInfo {
    pub txid: String,
    pub is_trace: bool,
}

#[derive(serde::Serialize)]
pub struct BlockDetails {
    pub height: u64,
    pub hash: String,
    pub time: Option<u64>,
    pub transactions: Vec<TransactionInfo>,
}

/// Get full block details including transactions from Bitcoin Core + Alkanes Trace info
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

    // 2. Get block details from Bitcoin Core
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
    let raw_txs = result.get("tx")
        .and_then(|t| t.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect::<Vec<String>>())
        .unwrap_or_default();

    // 3. Get Trace Indices from alkanes-cli (if available)
    // We try to fetch traces, but if it fails (e.g. no alkanes, or error), we just assume (is_trace=false).
    // 3. Get Trace Indices from Espo
    // (User requested to use Espo instead of alkanes-cli. Since the specific transaction-level trace endpoint
    // is not yet confirmed, we return an empty set to prevent the alkanes-cli panic.
    // TODO: Connect to valid Espo endpoint for tx-level trace data, e.g. /api/block/:height/traces)
    let trace_indices: std::collections::HashSet<usize> = std::collections::HashSet::new();

    // 4. Combine info
    let transactions = raw_txs.into_iter().enumerate().map(|(index, txid)| {
        let is_trace = trace_indices.contains(&index);
        TransactionInfo {
            txid,
            is_trace,
        }
    }).collect();

    Ok(BlockDetails {
        height,
        hash: hash.to_string(),
        time,
        transactions,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Alkanes Wallet API
// ─────────────────────────────────────────────────────────────────────────────

use crate::state::AlkanesWallet;

/// List all alkanes-cli wallets in ~/.alkanes/
#[tauri::command]
pub async fn get_alkanes_wallets() -> Result<Vec<AlkanesWallet>, String> {
    let home = std::env::var("HOME").map_err(|e| format!("Failed to get HOME: {}", e))?;
    let wallets_dir = std::path::Path::new(&home).join(".alkanes");

    if !wallets_dir.exists() {
        return Ok(Vec::new());
    }

    let mut wallets = Vec::new();
    let entries = std::fs::read_dir(wallets_dir)
        .map_err(|e| format!("Failed to read wallets dir: {}", e))?;

    for entry in entries {
        if let Ok(entry) = entry {
            let path = entry.path();
            // Only look for .json files
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    // Skip config.json as it's likely not a wallet
                    if stem == "config" {
                        continue;
                    }

                    wallets.push(AlkanesWallet {
                        name: stem.to_string(),
                        file_path: path.to_string_lossy().to_string(),
                        balance: None,
                        addresses: Vec::new(),
                    });
                }
            }
        }
    }
    
    // Sort by name
    wallets.sort_by(|a, b| a.name.cmp(&b.name));
    
    Ok(wallets)
}

/// Get details for a specific wallet via alkanes-cli
#[tauri::command]
pub async fn get_alkane_wallet_details(
    wallet_path: String,
    state: State<'_, SharedState>,
) -> Result<AlkanesWallet, String> {
    let path = std::path::Path::new(&wallet_path);
    let name = path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    // Check if bitcoind is running
    let bitcoind_running = {
        let mut state_guard = state.write().await;
        let status = state_guard.get_status();
        status.services.iter().any(|s| s.id == "bitcoind" && s.status == "running")
    };

    // 0. SYNC the wallet first (ONLY if bitcoind is running)
    if bitcoind_running {
        // This is required for the balance to be accurate (especially after confirmed txs).
        // We ignore errors here (logs them) because sometimes sync might fail but we still want to show what we have.
        let sync_res = std::process::Command::new("alkanes-cli")
            .arg("--wallet-file")
            .arg(&wallet_path)
            .arg("wallet")
            .arg("sync")
            .output();
            
        if let Err(e) = sync_res {
            tracing::warn!("Failed to sync wallet {}: {}", name, e);
        }
    } else {
        tracing::info!("Skipping wallet sync for {} (bitcoind not running)", name);
    }

    // 1. Get Addresses (index 0 of each address type)
    // alkanes-cli --wallet-file <path> wallet addresses
    let addr_output = std::process::Command::new("alkanes-cli")
        .args(["--wallet-file", &wallet_path, "wallet", "addresses"])
        .output()
        .map_err(|e| format!("Failed to run alkanes-cli: {}", e))?;

    let addresses: Vec<crate::state::AddressInfo> = if addr_output.status.success() {
        let out = String::from_utf8_lossy(&addr_output.stdout);
        let mut result = Vec::new();
        let mut current_section = "Unknown".to_string();
        
        for line in out.lines() {
            // Detect section headers like "📋 P2SH Addresses:"
            if line.contains("Addresses:") {
                // Extract "P2SH" from "📋 P2SH Addresses:"
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    // Usually "📋", "P2SH", "Addresses:"
                    // find the part before "Addresses:"
                    for (i, part) in parts.iter().enumerate() {
                        if part.contains("Addresses:") && i > 0 {
                            current_section = parts[i-1].to_string();
                            break;
                        }
                    }
                }
                continue;
            }
            
            // Look for "n." which marks index
            // Format: "  0. bcrt1xxx... (index: 0)"
            let trimmed = line.trim();
            if let Some(first_part) = trimmed.split('.').next() {
                if let Ok(idx_from_list) = first_part.parse::<usize>() {
                    // It likely starts with a number. Check if it contains an address.
                    // "0. address (index: 0)"
                    let parts: Vec<&str> = trimmed.splitn(2, ". ").collect();
                    if parts.len() == 2 {
                         // parts[1] is "address (index: 0)"
                         if let Some(addr_part) = parts[1].split(" (index:").next() {
                             let address = addr_part.trim().to_string();
                             
                             // We only want index 0 addresses for the main list?
                             // The previous logic filtered for found_index_0.
                             // But maybe we should collect ALL and filter later, or just collect index 0.
                             // The user said "I see 0, 1, 2, 3 for address". This implies they see MULTIPLE indices.
                             // IF we want to show multiple indices, we should collect them.
                             // BUT checking only index 0 reduces clutter.
                             // Let's stick to Index 0 for now as "Primary" for each type, UNLESS the user wants all.
                             // "I see 0, 1, 2, 3..." implies we ARE showing multiple or the UI is iterating.
                             // In WalletsPanel.tsx, we map `activeWallet.addresses`.
                             // If we only push index 0, the user only sees index 0.
                             
                             // Let's collect ONLY index 0 to avoid clutter, as per previous design.
                             // If the user saw "0, 1, 2, 3", maybe they meant "Address 0, Address 1" which were diff types?
                             // Yes, in previous logic we collected index 0 of P2SH, P2PKH, etc.
                             // So "Address 0" was P2SH, "Address 1" was P2PKH.
                             
                             // So we check if this is index 0.
                             if trimmed.contains("(index: 0)") {
                                 result.push(crate::state::AddressInfo {
                                     address,
                                     type_label: current_section.clone(),
                                     index: 0,
                                 });
                             }
                         }
                    }
                }
            }
        }
        
        // PRIORITIZE TAPROOT (P2TR) addresses at the top
        result.sort_by(|a, b| {
            let a_is_tr = a.type_label.contains("P2TR");
            let b_is_tr = b.type_label.contains("P2TR");
            b_is_tr.cmp(&a_is_tr)
        });
        
        result
    } else {
        Vec::new()
    };

    // 2. Get Balance by parsing UTXOs
    // alkanes-cli --wallet-file <path> wallet utxos
    let utxo_output = std::process::Command::new("alkanes-cli")
        .arg("--wallet-file")
        .arg(&wallet_path)
        .arg("wallet")
        .arg("utxos")
        .output();

    let balance = if let Ok(output) = utxo_output {
        if output.status.success() {
            let out = String::from_utf8_lossy(&output.stdout);
            struct CurrentUtxo {
                amount: u64,
                confs: u64,
                is_coinbase: bool,
                seen: bool,
            }
            let mut current = CurrentUtxo { amount: 0, confs: 0, is_coinbase: false, seen: false };
            let mut confirmed_sats: u64 = 0;
            let mut pending_sats: u64 = 0;

            // Helper to process a finished UTXO
            let mut process_utxo = |utxo: &CurrentUtxo, confirmed: &mut u64, pending: &mut u64| {
                if !utxo.seen { return; }
                // Filter immature coinbase (Regtest maturity is 100 blocks)
                if utxo.is_coinbase && utxo.confs < 100 {
                    return;
                }
                if utxo.confs > 0 {
                    *confirmed += utxo.amount;
                } else {
                    *pending += utxo.amount;
                }
            };

            for line in out.lines() {
                // Detect start of new UTXO (or end of previous) by "Outpoint:"
                if line.contains("Outpoint:") {
                    process_utxo(&current, &mut confirmed_sats, &mut pending_sats);
                    current = CurrentUtxo { amount: 0, confs: 0, is_coinbase: false, seen: true };
                }

                // Parse Amount: Filter non-digits to handle ANSI codes
                if line.contains("Amount (sats):") {
                    if let Some(rest) = line.split("Amount (sats):").nth(1) {
                         let digits: String = rest.chars().filter(|c| c.is_ascii_digit()).collect();
                         if let Ok(val) = digits.parse::<u64>() {
                             current.amount = val;
                         }
                    }
                }
                // Parse Confirmations
                if line.contains("Confirmations:") {
                    if let Some(rest) = line.split("Confirmations:").nth(1) {
                         let digits: String = rest.chars().filter(|c| c.is_ascii_digit()).collect();
                         if let Ok(val) = digits.parse::<u64>() {
                             current.confs = val;
                         }
                    }
                }
                // Parse Properties for coinbase
                if line.contains("Properties:") && line.to_lowercase().contains("coinbase") {
                    current.is_coinbase = true;
                }
            }
            // Process the last UTXO
            process_utxo(&current, &mut confirmed_sats, &mut pending_sats);
            
            let total_btc = (confirmed_sats as f64 + pending_sats as f64) / 100_000_000.0;
            Some(format!("{:.8} BTC", total_btc))
        } else {
            None
        }
    } else {
        None
    };

    Ok(AlkanesWallet {
        name,
        file_path: wallet_path,
        balance,
        addresses,
    })
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

    // 2. Mine a block to confirm
    // We need to keep a small delay or just mine immediately? 
    // Usually immediate is fine in regtest.
    // We pass None for address to use default miner address.
    match mine_blocks(1, None, state).await {
        Ok(_) => Ok(txid),
        Err(e) => Err(format!("Funds sent (txid: {}) but mining failed: {}", txid, e)),
    }
}
