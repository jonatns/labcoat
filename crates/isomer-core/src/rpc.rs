//! Bitcoin Core JSON-RPC helpers for the devnet
//!
//! Thin async client used by every frontend for chain queries, mining,
//! and the dev-wallet faucet. Extracted verbatim from the Tauri command
//! layer so the desktop app and the CLI share one code path.

use crate::config::IsomerConfig;
use serde::Serialize;

/// A block summary as shown in explorer carousels.
#[derive(Debug, Clone, Serialize)]
pub struct BlockSummary {
    pub height: u64,
    pub traces: u64,
    pub time: Option<u64>,
}

/// A transaction entry within [`BlockDetails`].
#[derive(Debug, Clone, Serialize)]
pub struct TransactionInfo {
    pub txid: String,
    pub is_trace: bool,
}

/// Full block details including transaction ids.
#[derive(Debug, Clone, Serialize)]
pub struct BlockDetails {
    pub height: u64,
    pub hash: String,
    pub time: Option<u64>,
    pub transactions: Vec<TransactionInfo>,
}

fn rpc_url(config: &IsomerConfig) -> String {
    format!("http://127.0.0.1:{}", config.ports.bitcoind_rpc)
}

fn client(timeout: std::time::Duration) -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .unwrap_or_default()
}

/// Perform a Bitcoin Core JSON-RPC call and return the `result` value.
pub async fn call(
    config: &IsomerConfig,
    wallet: Option<&str>,
    method: &str,
    params: serde_json::Value,
    timeout: std::time::Duration,
) -> Result<serde_json::Value, String> {
    let mut url = rpc_url(config);
    if let Some(wallet) = wallet {
        url = format!("{}/wallet/{}", url, wallet);
    }

    let response = client(timeout)
        .post(&url)
        .basic_auth(
            &config.bitcoind.rpc_user,
            Some(&config.bitcoind.rpc_password),
        )
        .json(&serde_json::json!({
            "jsonrpc": "1.0",
            "id": "isomer",
            "method": method,
            "params": params
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

    Ok(result.get("result").cloned().unwrap_or(serde_json::Value::Null))
}

/// Current block count, or None if bitcoind is unreachable.
pub async fn try_block_count(config: &IsomerConfig) -> Option<u64> {
    call(
        config,
        None,
        "getblockcount",
        serde_json::json!([]),
        std::time::Duration::from_millis(500),
    )
    .await
    .ok()
    .and_then(|v| v.as_u64())
}

/// Current mempool size, or None if bitcoind is unreachable.
pub async fn try_mempool_size(config: &IsomerConfig) -> Option<usize> {
    call(
        config,
        None,
        "getmempoolinfo",
        serde_json::json!([]),
        std::time::Duration::from_millis(500),
    )
    .await
    .ok()
    .and_then(|v| v.get("size").and_then(|s| s.as_u64()))
    .map(|s| s as usize)
}

/// Send BTC from the dev wallet to any address (the faucet).
/// Returns the txid. Amounts <= 0 default to 1 BTC.
pub async fn faucet(config: &IsomerConfig, address: &str, amount: f64) -> Result<String, String> {
    let amount_btc = if amount <= 0.0 { 1.0 } else { amount };

    let result = call(
        config,
        Some("dev"),
        "sendtoaddress",
        serde_json::json!([address, amount_btc]),
        std::time::Duration::from_secs(30),
    )
    .await
    .map_err(|e| format!("Faucet error: {}", e))?;

    let txid = result.as_str().unwrap_or("unknown").to_string();

    tracing::info!(
        "Faucet: sent {} BTC to {} (txid: {})",
        amount_btc,
        address,
        txid
    );

    Ok(txid)
}

/// Fallback mine-to address when no account or explicit address exists.
pub const DEFAULT_MINE_ADDRESS: &str = "bcrt1q9zuctyd46l7sdedccdk47335lzsmjz2wngdv3u";

/// Mine `count` blocks to `address` and return the new block height.
pub async fn mine_blocks(
    config: &IsomerConfig,
    count: u32,
    address: &str,
) -> Result<u64, String> {
    if count > 1000 {
        return Err("Cannot mine more than 1000 blocks at once.".to_string());
    }

    call(
        config,
        None,
        "generatetoaddress",
        serde_json::json!([count, address]),
        std::time::Duration::from_secs(60),
    )
    .await?;

    let height = call(
        config,
        None,
        "getblockcount",
        serde_json::json!([]),
        std::time::Duration::from_secs(10),
    )
    .await?
    .as_u64()
    .unwrap_or(0);

    Ok(height)
}

/// Latest block info directly from Bitcoin Core (for optimistic UI updates).
pub async fn latest_block(config: &IsomerConfig) -> Result<BlockSummary, String> {
    let timeout = std::time::Duration::from_millis(500);

    let height = call(config, None, "getblockcount", serde_json::json!([]), timeout)
        .await
        .map_err(|e| format!("Failed to connect to Bitcoin Core: {}", e))?
        .as_u64()
        .ok_or("Invalid block count response")?;

    let hash = call(
        config,
        None,
        "getblockhash",
        serde_json::json!([height]),
        timeout,
    )
    .await
    .map_err(|e| format!("Failed to get block hash: {}", e))?;
    let hash = hash.as_str().ok_or("Invalid block hash response")?;

    let block = call(config, None, "getblock", serde_json::json!([hash]), timeout)
        .await
        .map_err(|e| format!("Failed to get block details: {}", e))?;
    let time = block.get("time").and_then(|t| t.as_u64());

    Ok(BlockSummary {
        height,
        traces: 0, // Bitcoin Core doesn't know about traces, Espo will fill this later
        time,
    })
}

/// Full block details including transactions from Bitcoin Core.
pub async fn block_details(config: &IsomerConfig, height: u64) -> Result<BlockDetails, String> {
    let timeout = std::time::Duration::from_millis(1000);

    let hash = call(
        config,
        None,
        "getblockhash",
        serde_json::json!([height]),
        timeout,
    )
    .await
    .map_err(|e| format!("Failed to get block hash: {}", e))?;
    let hash = hash
        .as_str()
        .ok_or_else(|| format!("Block not found at height {}", height))?
        .to_string();

    // 1 for JSON with txids
    let block = call(
        config,
        None,
        "getblock",
        serde_json::json!([hash, 1]),
        timeout,
    )
    .await
    .map_err(|e| format!("Failed to get block details: {}", e))?;

    let time = block.get("time").and_then(|t| t.as_u64());
    let raw_txs = block
        .get("tx")
        .and_then(|t| t.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect::<Vec<String>>()
        })
        .unwrap_or_default();

    // Trace indices are not yet sourced (Espo tx-level trace endpoint TBD);
    // mirror the previous behavior of marking none.
    let transactions = raw_txs
        .into_iter()
        .map(|txid| TransactionInfo {
            txid,
            is_trace: false,
        })
        .collect();

    Ok(BlockDetails {
        height,
        hash,
        time,
        transactions,
    })
}
