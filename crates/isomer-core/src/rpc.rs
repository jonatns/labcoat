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
    let response =
        exchange_blocking(config, method, params, timeout).map_err(|error| match error {
            Exchange::Refused => "Labcoat Network RPC call failed: connection refused".to_string(),
            Exchange::TimedOut => "Labcoat Network RPC call timed out".to_string(),
            Exchange::Failed(message) => message,
        })?;
    result_from_response(response)
}

/// Why a blocking exchange produced no response.
enum Exchange {
    /// Nothing is listening on the port.
    Refused,
    /// Something accepted the connection but did not answer in time.
    TimedOut,
    Failed(String),
}

/// Send one request and return the parsed JSON-RPC response object.
fn exchange_blocking(
    config: &IsomerConfig,
    method: &str,
    params: serde_json::Value,
    timeout: std::time::Duration,
) -> Result<serde_json::Value, Exchange> {
    use std::io::{ErrorKind, Read, Write};

    let classify = |error: std::io::Error, doing: &str| match error.kind() {
        ErrorKind::ConnectionRefused => Exchange::Refused,
        ErrorKind::TimedOut | ErrorKind::WouldBlock => Exchange::TimedOut,
        _ => Exchange::Failed(format!("Labcoat Network RPC {doing} failed: {error}")),
    };
    let mut stream = std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], config.ports.rpc)),
        timeout,
    )
    .map_err(|e| classify(e, "connection"))?;
    stream
        .set_read_timeout(Some(timeout))
        .and_then(|()| stream.set_write_timeout(Some(timeout)))
        .map_err(|e| classify(e, "setup"))?;
    let body = serde_json::to_vec(&request_body(method, params)).map_err(|e| {
        Exchange::Failed(format!("Failed to encode Labcoat Network RPC request: {e}"))
    })?;
    write!(
        stream,
        "POST / HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .and_then(|_| stream.write_all(&body))
    .map_err(|e| classify(e, "request"))?;
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|e| classify(e, "response"))?;
    let invalid = || Exchange::Failed("Invalid Labcoat Network HTTP response".to_string());
    let body_start = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|position| position + 4)
        .ok_or_else(invalid)?;
    serde_json::from_slice(&response[body_start..]).map_err(|_| invalid())
}

/// What is on Labcoat Network's port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeIdentity {
    /// This release's node, serving this data home.
    Own,
    /// Another release's node, serving this data home. It is this data
    /// home's network, so it may be stopped and replaced.
    OwnOtherRelease,
    /// A Labcoat Network serving another data home, named here. It belongs
    /// to another project or session and is never stopped from this one.
    OtherDataHome(String),
    /// Something else answered. Labcoat cannot tell whose it is, so it is
    /// never asked to stop.
    Foreign,
    /// Nothing is listening.
    Absent,
    /// Something accepted the connection but did not answer in time. It is
    /// left alone: it may be a node that is still working.
    Unresponsive,
}

/// Ask whoever is on the port who it is.
pub fn identify_blocking(config: &IsomerConfig, timeout: std::time::Duration) -> NodeIdentity {
    match exchange_blocking(config, "labcoat_info", serde_json::json!([]), timeout) {
        Ok(response) => match result_from_response(response) {
            Ok(info) if is_own_node_info(&info) => NodeIdentity::Own,
            Ok(info) if serves_this_data_home(&info) => NodeIdentity::OwnOtherRelease,
            Ok(info) => match other_data_home(&info) {
                Some(data_dir) => NodeIdentity::OtherDataHome(data_dir),
                None => NodeIdentity::Foreign,
            },
            Err(_) => NodeIdentity::Foreign,
        },
        Err(Exchange::Refused) => NodeIdentity::Absent,
        Err(Exchange::TimedOut) => NodeIdentity::Unresponsive,
        Err(Exchange::Failed(_)) => NodeIdentity::Foreign,
    }
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

/// Whether `labcoat_info` describes a Labcoat Network serving this data
/// home, whatever its release.
fn serves_this_data_home(info: &serde_json::Value) -> bool {
    let own = crate::config::get_network_dir().display().to_string();
    info.get("name").and_then(serde_json::Value::as_str) == Some("Labcoat Network")
        && info.get("dataDir").and_then(serde_json::Value::as_str) == Some(own.as_str())
}

/// The data directory of a Labcoat Network that serves a data home other
/// than this one, whatever its release.
fn other_data_home(info: &serde_json::Value) -> Option<String> {
    let data_dir = info.get("dataDir").and_then(serde_json::Value::as_str)?;
    (info.get("name").and_then(serde_json::Value::as_str) == Some("Labcoat Network")
        && !serves_this_data_home(info))
    .then(|| data_dir.to_string())
}

/// Whether this release's node is serving this data home on the port.
pub fn is_own_node_blocking(config: &IsomerConfig) -> bool {
    identify_blocking(config, std::time::Duration::from_millis(500)) == NodeIdentity::Own
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

/// Blocks requested from the node per `generatetoaddress` call, so that no
/// single request outlives its timeout.
const MINE_BATCH: u32 = 1_000;
/// Most blocks one `mine` may produce: about two years of Bitcoin blocks.
pub const MAX_MINE_BLOCKS: u32 = 100_000;

fn mine_batches(count: u32) -> impl Iterator<Item = u32> {
    (0..count)
        .step_by(MINE_BATCH as usize)
        .map(move |mined| MINE_BATCH.min(count - mined))
}

pub async fn mine_blocks(config: &IsomerConfig, count: u32, address: &str) -> Result<u64, String> {
    if count > MAX_MINE_BLOCKS {
        return Err(format!(
            "Cannot mine more than {MAX_MINE_BLOCKS} blocks at once"
        ));
    }
    for batch in mine_batches(count) {
        call(
            config,
            "generatetoaddress",
            serde_json::json!([batch, address]),
            std::time::Duration::from_secs(60),
        )
        .await?;
    }
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
    fn mining_is_split_into_bounded_batches() {
        assert_eq!(mine_batches(0).count(), 0);
        assert_eq!(mine_batches(1).collect::<Vec<_>>(), [1]);
        assert_eq!(mine_batches(1_000).collect::<Vec<_>>(), [1_000]);
        assert_eq!(
            mine_batches(3_456).collect::<Vec<_>>(),
            [1_000, 1_000, 1_000, 456]
        );
        assert_eq!(mine_batches(MAX_MINE_BLOCKS).sum::<u32>(), MAX_MINE_BLOCKS);
    }

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

    /// A config whose port serves `respond` once per connection, or nothing.
    fn serve(respond: Option<&'static str>) -> IsomerConfig {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let mut config = IsomerConfig::default();
        config.ports.rpc = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let Some(body) = respond else {
                    // Accept and hold the connection without answering.
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_secs(5));
                        drop(stream);
                    });
                    continue;
                };
                use std::io::Write;
                // Read the whole request, as a real server does: answering
                // before the body arrives and closing with it unread makes
                // the kernel reset the connection.
                read_request(&mut stream);
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        config
    }

    fn read_request(stream: &mut std::net::TcpStream) {
        use std::io::Read;
        let mut request = Vec::new();
        let mut chunk = [0u8; 1024];
        let headers_end = loop {
            if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                break end + 4;
            }
            match stream.read(&mut chunk) {
                Ok(0) | Err(_) => return,
                Ok(read) => request.extend_from_slice(&chunk[..read]),
            }
        };
        let headers = String::from_utf8_lossy(&request[..headers_end]).to_ascii_lowercase();
        let length: usize = headers
            .lines()
            .find_map(|line| line.strip_prefix("content-length:"))
            .and_then(|value| value.trim().parse().ok())
            .unwrap_or(0);
        while request.len() < headers_end + length {
            match stream.read(&mut chunk) {
                Ok(0) | Err(_) => return,
                Ok(read) => request.extend_from_slice(&chunk[..read]),
            }
        }
    }

    #[test]
    fn tells_own_foreign_absent_and_unresponsive_nodes_apart() {
        let short = std::time::Duration::from_millis(300);

        let unused = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let mut absent = IsomerConfig::default();
        absent.ports.rpc = unused.local_addr().unwrap().port();
        drop(unused);
        assert_eq!(identify_blocking(&absent, short), NodeIdentity::Absent);

        assert_eq!(
            identify_blocking(&serve(None), short),
            NodeIdentity::Unresponsive
        );

        let qubitcoind = serve(Some(
            r#"{"jsonrpc":"2.0","error":{"code":-32601,"message":"Method not found: labcoat_info"},"id":1}"#,
        ));
        assert_eq!(identify_blocking(&qubitcoind, short), NodeIdentity::Foreign);

        let own = format!(
            r#"{{"jsonrpc":"2.0","result":{{"name":"Labcoat Network","version":"{}","dataDir":{}}},"id":1}}"#,
            env!("CARGO_PKG_VERSION"),
            serde_json::to_string(&crate::config::get_network_dir().display().to_string()).unwrap()
        );
        assert_eq!(
            identify_blocking(&serve(Some(Box::leak(own.into_boxed_str()))), short),
            NodeIdentity::Own
        );

        // Another data home's network is told apart from other programs,
        // whatever release serves it, so it can be left running.
        let other_home = r#"{"jsonrpc":"2.0","result":{"name":"Labcoat Network","version":"0.0.1","dataDir":"/another/home/network"},"id":1}"#;
        assert_eq!(
            identify_blocking(&serve(Some(other_home)), short),
            NodeIdentity::OtherDataHome("/another/home/network".into())
        );

        // An earlier release's node for this data home is recognized as
        // this data home's, so it can be replaced.
        let earlier = format!(
            r#"{{"jsonrpc":"2.0","result":{{"name":"Labcoat Network","version":"0.0.1","dataDir":{}}},"id":1}}"#,
            serde_json::to_string(&crate::config::get_network_dir().display().to_string()).unwrap()
        );
        assert_eq!(
            identify_blocking(&serve(Some(Box::leak(earlier.into_boxed_str()))), short),
            NodeIdentity::OwnOtherRelease
        );
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
