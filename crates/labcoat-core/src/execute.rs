//! Execute (call) and deploy against a devnet/network, via the pinned
//! alkanes-rs executor — commit/reveal envelope deploys included.

use crate::error::{LabcoatError, Result};
use crate::system::ToolkitConfig;
use alkanes_cli_common::alkanes::execute::EnhancedAlkanesExecutor;
use alkanes_cli_common::alkanes::types::{
    EnhancedExecuteParams, EnhancedExecuteResult, OrdinalsStrategy, UtxoDataSource,
};
use alkanes_cli_common::provider::ConcreteProvider;
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteOutcome {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_txid: Option<String>,
    pub txid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_fee: Option<u64>,
    pub fee: u64,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revert_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alkanes_id: Option<String>,
    pub traces: Option<Vec<serde_json::Value>>,
}

/// Build the standard cellpack protostone spec string:
/// `[block,tx,opcode,args…]:v0:v0` (pointer/refund to output 0).
pub fn cellpack_spec(block: u128, tx: u128, opcode: u128, args: &[u128]) -> String {
    let mut inputs = vec![block.to_string(), tx.to_string(), opcode.to_string()];
    inputs.extend(args.iter().map(|a| a.to_string()));
    format!("[{}]:v0:v0", inputs.join(","))
}

/// Run the executor with a cellpack spec, optional envelope (deploy), and
/// standard labcoat behavior: auto-confirm, trace, auto-mine on regtest,
/// UTXOs filtered to the indexer height.
pub async fn run(
    provider: &mut ConcreteProvider,
    config: &ToolkitConfig,
    protostones_spec: &str,
    envelope_data: Option<Vec<u8>>,
    to_address: String,
    fee_rate: Option<f32>,
    max_indexed_height: Option<u64>,
) -> Result<EnhancedExecuteResult> {
    let protostones = alkanes_cli_common::alkanes::parsing::parse_protostones(protostones_spec)
        .map_err(|e| {
            LabcoatError::new(
                "ENVELOPE_INVALID",
                format!("bad protostone spec '{}': {}", protostones_spec, e),
                "expected [block,tx,opcode,args...]:v0:v0",
            )
        })?;

    let mine_enabled = config.normalized_network() == "regtest";

    let params = EnhancedExecuteParams {
        fee_rate: fee_rate.or(config.fee_rate),
        to_addresses: vec![to_address],
        from_addresses: None,
        change_address: None,
        alkanes_change_address: None,
        input_requirements: Vec::new(),
        protostones,
        envelope_data,
        raw_output: true,
        trace_enabled: true,
        mine_enabled,
        auto_confirm: true,
        ordinals_strategy: OrdinalsStrategy::default(),
        mempool_indexer: false,
        split_transactions: false,
        known_pending_tx_hexes: Vec::new(),
        prefetched_utxos: Vec::new(),
        excluded_utxos: Vec::new(),
        max_indexed_height,
        utxo_source: UtxoDataSource::default(),
    };

    let mut executor = EnhancedAlkanesExecutor::new(provider);
    executor
        .execute_full(params)
        .await
        .map_err(|e| LabcoatError::classify(e.into()))
}

/// Extract `block:tx` of a newly created alkane from trace events.
pub fn find_created_alkane(traces: &Option<Vec<serde_json::Value>>) -> Option<String> {
    let traces = traces.as_ref()?;
    for trace in traces {
        if let Some(id) = scan_for_create(trace) {
            return Some(id);
        }
    }
    None
}

fn scan_for_create(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::Object(map) => {
            // Shapes seen from trace_to_json: {"event":"create","data":{"block":N,"tx":M}}
            // and {"type":"create_alkane","alkane_id"/"new_alkane":{"block":..,"tx":..}}
            let is_create = map
                .get("event")
                .and_then(|e| e.as_str())
                .map(|e| e == "create")
                .unwrap_or(false)
                || map
                    .get("type")
                    .and_then(|t| t.as_str())
                    .map(|t| t == "create_alkane")
                    .unwrap_or(false);
            if is_create {
                for key in ["data", "alkane_id", "new_alkane"] {
                    if let Some(idv) = map.get(key) {
                        if let Some(id) = extract_id(idv) {
                            return Some(id);
                        }
                    }
                }
            }
            for v in map.values() {
                if let Some(found) = scan_for_create(v) {
                    return Some(found);
                }
            }
            None
        }
        serde_json::Value::Array(items) => items.iter().find_map(scan_for_create),
        _ => None,
    }
}

fn extract_id(value: &serde_json::Value) -> Option<String> {
    let block = value.get("block")?;
    let tx = value.get("tx")?;
    let to_num = |v: &serde_json::Value| -> Option<u128> {
        if let Some(n) = v.as_u64() {
            return Some(n as u128);
        }
        let s = v.as_str()?;
        if let Some(hex) = s.strip_prefix("0x") {
            u128::from_str_radix(hex, 16).ok()
        } else {
            s.parse().ok()
        }
    };
    Some(format!("{}:{}", to_num(block)?, to_num(tx)?))
}

/// Extract the return status ("success" | "revert" | "unknown") and any
/// decoded revert reason from trace events.
pub fn find_return_status(traces: &Option<Vec<serde_json::Value>>) -> (String, Option<String>) {
    let Some(traces) = traces else {
        return ("unknown".to_string(), None);
    };
    for trace in traces {
        if let Some(found) = scan_for_return(trace) {
            return found;
        }
    }
    ("unknown".to_string(), None)
}

fn scan_for_return(value: &serde_json::Value) -> Option<(String, Option<String>)> {
    match value {
        serde_json::Value::Object(map) => {
            let event = map.get("event").and_then(|e| e.as_str());
            let typ = map.get("type").and_then(|t| t.as_str());
            if event == Some("return") || typ == Some("return") || typ == Some("revert") {
                let data = map.get("data");
                let status = data
                    .and_then(|d| d.get("status"))
                    .and_then(|s| s.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| {
                        if typ == Some("revert") {
                            "revert".to_string()
                        } else {
                            "success".to_string()
                        }
                    });
                let reason = map
                    .get("error_message")
                    .or_else(|| data.and_then(|d| d.get("error_message")))
                    .and_then(|m| m.as_str())
                    .map(|s| s.to_string())
                    .or_else(|| {
                        data.and_then(|d| d.get("response"))
                            .and_then(|r| r.get("data"))
                            .and_then(|d| d.as_str())
                            .and_then(decode_revert_reason)
                    });
                return Some((status, reason));
            }
            map.values().find_map(scan_for_return)
        }
        serde_json::Value::Array(items) => items.iter().find_map(scan_for_return),
        _ => None,
    }
}

/// Same semantics as the old TS decodeRevertReason: skip "0x" + 4-byte
/// selector, interpret the rest as UTF-8.
pub fn decode_revert_reason(hex_str: &str) -> Option<String> {
    if hex_str.is_empty() || hex_str == "0x" {
        return None;
    }
    let data = hex_str.strip_prefix("0x").unwrap_or(hex_str);
    if data.len() <= 8 {
        return None;
    }
    let bytes = hex::decode(&data[8..]).ok()?;
    String::from_utf8(bytes).ok()
}

#[cfg(test)]
mod tests {
    #[test]
    fn serde_json_preserves_full_u128_cellpack_values() {
        let value = serde_json::json!(u128::MAX);
        assert_eq!(value.to_string(), u128::MAX.to_string());
    }
}
