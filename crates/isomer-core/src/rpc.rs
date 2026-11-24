//! JSON-RPC helpers for Labcoat Network.

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
    format!("http://127.0.0.1:{}", config.ports.rpc)
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
        return Err(format!("Labcoat Network RPC error: {message}"));
    }
    response
        .get("result")
        .cloned()
        .ok_or_else(|| "Labcoat Network RPC response is missing result".to_string())
}

/// One client for the whole process: building a client loads the TLS root
/// certificates, which costs tens of milliseconds every time.
fn client() -> Result<&'static reqwest::Client, String> {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    if let Some(client) = CLIENT.get() {
        return Ok(client);
    }
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .map_err(|e| format!("Failed to create Labcoat Network RPC client: {e}"))?;
    Ok(CLIENT.get_or_init(|| client))
}

pub async fn call(
    config: &IsomerConfig,
    method: &str,
    params: serde_json::Value,
    timeout: std::time::Duration,
) -> Result<serde_json::Value, String> {
    let response = client()?
        .post(rpc_url(config))
        .timeout(timeout)
        .json(&request_body(method, params))
        .send()
        .await
        .map_err(|e| format!("Labcoat Network RPC call failed: {e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Labcoat Network RPC returned HTTP {}",
            response.status()
        ));
    }
    result_from_response(
        response
            .json()
            .await
            .map_err(|e| format!("Failed to parse Labcoat Network RPC response: {e}"))?,
    )
}

pub fn call_blocking(
    config: &IsomerConfig,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    call_blocking_with_timeout(config, method, params, std::time::Duration::from_secs(60))
}

pub fn call_blocking_with_timeout(
    config: &IsomerConfig,
    method: &str,
    params: serde_json::Value,
    timeout: std::time::Duration,
) -> Result<serde_json::Value, String> {
    use std::io::{Read, Write};

    let mut stream = std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], config.ports.rpc)),
        timeout,
    )
    .map_err(|e| format!("Labcoat Network RPC call failed: {e}"))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| format!("Failed to configure Labcoat Network RPC: {e}"))?;
    let body = serde_json::to_vec(&request_body(method, params))
        .map_err(|e| format!("Failed to encode Labcoat Network RPC request: {e}"))?;
    write!(
        stream,
        "POST / HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .and_then(|_| stream.write_all(&body))
    .map_err(|e| format!("Labcoat Network RPC call failed: {e}"))?;
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|e| format!("Failed to read Labcoat Network RPC response: {e}"))?;
    let body_start = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|position| position + 4)
        .ok_or_else(|| "Invalid Labcoat Network HTTP response".to_string())?;
    result_from_response(
        serde_json::from_slice(&response[body_start..])
            .map_err(|e| format!("Failed to parse Labcoat Network RPC response: {e}"))?,
    )
}

/// Whether `labcoat_info` describes this release's node serving this data
/// home. Anything else on the port, such as a `qubitcoind` left by an older
/// release, a node from another release, or one serving another
/// `LABCOAT_DATA_HOME`, is not reused or trusted.
fn is_own_node_info(info: &serde_json::Value) -> bool {
    let data_dir = crate::config::get_network_dir().display().to_string();
    info.get("name").and_then(serde_json::Value::as_str) == Some("Labcoat Network")
        && info.get("version").and_then(serde_json::Value::as_str)
            == Some(env!("CARGO_PKG_VERSION"))
        && info.get("dataDir").and_then(serde_json::Value::as_str) == Some(data_dir.as_str())
}

/// Whether this release's node is serving this data home on the port.
pub fn is_own_node_blocking(config: &IsomerConfig) -> bool {
    call_blocking_with_timeout(
        config,
        "labcoat_info",
        serde_json::json!([]),
        std::time::Duration::from_millis(500),
    )
    .is_ok_and(|info| is_own_node_info(&info))
}

/// Whether this release's node is serving this data home on the port.
pub async fn is_own_node(config: &IsomerConfig) -> bool {
    call(
        config,
        "labcoat_info",
        serde_json::json!([]),
        std::time::Duration::from_secs(2),
    )
    .await
    .is_ok_and(|info| is_own_node_info(&info))
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
    .ok_or_else(|| "Invalid Labcoat Network block count response".to_string())
}

pub async fn latest_block(config: &IsomerConfig) -> Result<BlockSummary, String> {
    let timeout = std::time::Duration::from_secs(1);
    let height = call(config, "getblockcount", serde_json::json!([]), timeout)
        .await?
        .as_u64()
        .ok_or_else(|| "Invalid Labcoat Network block count response".to_string())?;
    let hash = call(config, "getblockhash", serde_json::json!([height]), timeout)
        .await?
        .as_str()
        .ok_or_else(|| "Invalid Labcoat Network block hash response".to_string())?
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
    fn only_this_releases_node_for_this_data_home_is_trusted() {
        let own = serde_json::json!({
            "name": "Labcoat Network",
            "version": env!("CARGO_PKG_VERSION"),
            "dataDir": crate::config::get_network_dir().display().to_string(),
        });
        assert!(is_own_node_info(&own));

        let mut other_release = own.clone();
        other_release["version"] = "0.0.1".into();
        let mut other_home = own.clone();
        other_home["dataDir"] = "/somewhere/else".into();
        for foreign in [
            other_release,
            other_home,
            serde_json::json!({"chain": "regtest"}),
        ] {
            assert!(!is_own_node_info(&foreign), "{foreign}");
        }
    }

    #[test]
    fn endpoint_is_the_single_rpc_port() {
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
        assert_eq!(error, "Labcoat Network RPC error: boom");
    }
}
