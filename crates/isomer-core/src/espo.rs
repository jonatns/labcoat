//! Espo explorer/indexer API client
//!
//! Queries the local Espo instance for deployed alkanes and explorer
//! block data. Extracted from the Tauri command layer; ports now come
//! from config instead of being hardcoded.

use crate::config::IsomerConfig;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct EspoCarouselBlock {
    pub height: u64,
    pub traces: u64,
    pub time: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EspoCarouselResponse {
    pub espo_tip: u64,
    pub blocks: Vec<EspoCarouselBlock>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlkaneInfo {
    pub alkane: String,
    pub creation_txid: String,
    pub creation_height: u64,
    pub creation_timestamp: Option<u64>,
    pub name: Option<String>,
    pub symbol: Option<String>,
    pub holder_count: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct EspoAlkanesResponse {
    pub ok: bool,
    pub page: u64,
    pub limit: u64,
    pub total: u64,
    pub items: Vec<AlkaneInfo>,
}

/// Fetch all deployed alkanes from the Espo RPC API.
pub async fn get_all_alkanes(
    config: &IsomerConfig,
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

    let url = format!("http://127.0.0.1:{}/rpc", config.ports.espo_rpc);
    let response = client
        .post(&url)
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
                        name: item
                            .get("name")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        symbol: item
                            .get("symbol")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        holder_count: item
                            .get("holder_count")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0),
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

/// Fetch carousel blocks from the Espo explorer API.
pub async fn get_espo_blocks(
    config: &IsomerConfig,
    center: Option<u64>,
    radius: Option<u64>,
) -> Result<EspoCarouselResponse, String> {
    let radius = radius.unwrap_or(10);
    let mut url = format!(
        "http://127.0.0.1:{}/api/blocks/carousel?radius={}",
        config.ports.espo_explorer, radius
    );

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

    let espo_tip = json.get("espo_tip").and_then(|v| v.as_u64()).unwrap_or(0);

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
