//! Direct Qubitcoin JSON-RPC helpers for Labcoat Network.

use crate::config::IsomerConfig;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct BlockSummary {
    pub height: u64,
    pub traces: u64,
    pub time: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TransactionInfo {
    pub txid: String,
    pub is_trace: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct BlockDetails {
    pub height: u64,
    pub hash: String,
    pub time: Option<u64>,
    pub transactions: Vec<TransactionInfo>,
}

pub fn rpc_url(config: &IsomerConfig) -> String {
    format!("http://127.0.0.1:{}", config.ports.qubitcoin_rpc)
}

fn request_body(method: &str, params: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": "labcoat",
        "method": method,
        "params": params
    })
}

fn result_from_response(response: serde_json::Value) -> Result<serde_json::Value, String> {
    if let Some(error) = response.get("error").filter(|value| !value.is_null()) {
        let message = error
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown error");
        return Err(format!("Qubitcoin RPC error: {message}"));
    }
    response
        .get("result")
        .cloned()
        .ok_or_else(|| "Qubitcoin RPC response is missing result".to_string())
}

pub async fn call(
    config: &IsomerConfig,
    method: &str,
    params: serde_json::Value,
    timeout: std::time::Duration,
) -> Result<serde_json::Value, String> {
    let response = reqwest::Client::builder()
        .no_proxy()
        .timeout(timeout)
        .build()
        .map_err(|e| format!("Failed to create Qubitcoin RPC client: {e}"))?
        .post(rpc_url(config))
        .json(&request_body(method, params))
        .send()
        .await
        .map_err(|e| format!("Qubitcoin RPC call failed: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("Qubitcoin RPC returned HTTP {}", response.status()));
    }
    result_from_response(
        response
            .json()
            .await
            .map_err(|e| format!("Failed to parse Qubitcoin RPC response: {e}"))?,
    )
}

pub fn call_blocking(
    config: &IsomerConfig,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use std::io::{Read, Write};

    let timeout = std::time::Duration::from_secs(60);
    let mut stream = std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], config.ports.qubitcoin_rpc)),
        timeout,
    )
    .map_err(|e| format!("Qubitcoin RPC call failed: {e}"))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| format!("Failed to configure Qubitcoin RPC: {e}"))?;
    let body = serde_json::to_vec(&request_body(method, params))
        .map_err(|e| format!("Failed to encode Qubitcoin RPC request: {e}"))?;
    write!(
        stream,
        "POST / HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .and_then(|_| stream.write_all(&body))
    .map_err(|e| format!("Qubitcoin RPC call failed: {e}"))?;
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|e| format!("Failed to read Qubitcoin RPC response: {e}"))?;
    let body_start = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|position| position + 4)
        .ok_or_else(|| "Invalid Qubitcoin HTTP response".to_string())?;
    result_from_response(
        serde_json::from_slice(&response[body_start..])
            .map_err(|e| format!("Failed to parse Qubitcoin RPC response: {e}"))?,
    )
}

pub async fn try_block_count(config: &IsomerConfig) -> Option<u64> {
    call(
        config,
        "getblockcount",
        serde_json::json!([]),
        std::time::Duration::from_millis(500),
    )
    .await
    .ok()
    .and_then(|value| value.as_u64())
}

pub async fn try_mempool_size(config: &IsomerConfig) -> Option<usize> {
    call(
        config,
        "getmempoolinfo",
        serde_json::json!([]),
        std::time::Duration::from_millis(500),
    )
    .await
    .ok()
    .and_then(|value| value.get("size").and_then(serde_json::Value::as_u64))
    .map(|size| size as usize)
}

pub async fn mine_blocks(config: &IsomerConfig, count: u32, address: &str) -> Result<u64, String> {
    if count > 1000 {
        return Err("Cannot mine more than 1000 blocks at once".to_string());
    }
    call(
        config,
        "generatetoaddress",
        serde_json::json!([count, address]),
        std::time::Duration::from_secs(60),
    )
    .await?;
    call(
        config,
        "getblockcount",
        serde_json::json!([]),
        std::time::Duration::from_secs(10),
    )
    .await?
    .as_u64()
    .ok_or_else(|| "Invalid Qubitcoin block count response".to_string())
}

pub async fn latest_block(config: &IsomerConfig) -> Result<BlockSummary, String> {
    let timeout = std::time::Duration::from_secs(1);
    let height = call(config, "getblockcount", serde_json::json!([]), timeout)
        .await?
        .as_u64()
        .ok_or_else(|| "Invalid Qubitcoin block count response".to_string())?;
    let hash = call(config, "getblockhash", serde_json::json!([height]), timeout)
        .await?
        .as_str()
        .ok_or_else(|| "Invalid Qubitcoin block hash response".to_string())?
        .to_string();
    let block = call(config, "getblock", serde_json::json!([hash]), timeout).await?;
    Ok(BlockSummary {
        height,
        traces: 0,
        time: block.get("time").and_then(serde_json::Value::as_u64),
    })
}

pub async fn block_details(config: &IsomerConfig, height: u64) -> Result<BlockDetails, String> {
    let timeout = std::time::Duration::from_secs(1);
    let hash = call(config, "getblockhash", serde_json::json!([height]), timeout)
        .await?
        .as_str()
        .ok_or_else(|| format!("Block not found at height {height}"))?
        .to_string();
    let block = call(config, "getblock", serde_json::json!([hash, 1]), timeout).await?;
    let transactions = block
        .get("tx")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(|txid| TransactionInfo {
            txid: txid.to_string(),
            is_trace: false,
        })
        .collect();
    Ok(BlockDetails {
        height,
        hash,
        time: block.get("time").and_then(serde_json::Value::as_u64),
        transactions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_is_the_single_qubitcoin_rpc() {
        let config = IsomerConfig::default();
        assert_eq!(rpc_url(&config), "http://127.0.0.1:18443");
    }

    #[test]
    fn reports_rpc_errors() {
        let error = result_from_response(serde_json::json!({
            "result": null,
            "error": {"message": "boom"}
        }))
        .unwrap_err();
        assert_eq!(error, "Qubitcoin RPC error: boom");
    }
}
