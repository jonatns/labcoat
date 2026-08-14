//! Transaction trace fetching + bounded wait.
//!
//! `trace_protostones` computes protostone vouts from the transaction output
//! count instead of assuming a fixed index.

use crate::error::{LabcoatError, Result};
use alkanes_cli_common::provider::ConcreteProvider;
use alkanes_cli_common::traits::{AlkanesProvider, JsonRpcProvider};
use prost::Message;

/// Fetch decoded traces for every protostone in a transaction.
/// Returns None when the tx carries no protostones.
pub async fn trace(
    provider: &ConcreteProvider,
    txid: &str,
) -> Result<Option<Vec<serde_json::Value>>> {
    if provider.rpc_config.is_qubitcoin_mode() {
        return trace_qubitcoin(provider, txid).await;
    }
    provider
        .trace_protostones(txid)
        .await
        .map_err(|e| LabcoatError::classify(e.into()))
}

/// Poll until traces exist for the tx (the indexer may lag the broadcast),
/// with a bounded timeout.
pub async fn wait_for_trace(
    provider: &ConcreteProvider,
    txid: &str,
    timeout: std::time::Duration,
) -> Result<Vec<serde_json::Value>> {
    let started = std::time::Instant::now();
    loop {
        match trace(provider, txid).await {
            Ok(Some(traces)) if !traces.is_empty() => {
                // An empty trace body means the indexer hasn't executed the
                // protostone yet; require at least one non-empty entry.
                let has_events = traces
                    .iter()
                    .any(|t| t.as_array().map(|a| !a.is_empty()).unwrap_or(true) && !t.is_null());
                if has_events {
                    return Ok(traces);
                }
            }
            Ok(_) => {}
            Err(e) => {
                tracing::debug!("trace not ready for {}: {}", txid, e);
            }
        }
        if started.elapsed() > timeout {
            return Err(LabcoatError::new(
                "TRACE_TIMEOUT",
                format!("no trace for {txid} after {timeout:?}"),
                "is the Alkanes index synced? run `labcoat status`, then `labcoat trace <txid>`",
            ));
        }
        tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
    }
}

/// Qubitcoin does not keep a Bitcoin Core-style transaction index. Read the
/// reveal transaction from its Esplora secondary, then query Alkanes traces
/// for the virtual protostone outpoints.
async fn trace_qubitcoin(
    provider: &ConcreteProvider,
    txid: &str,
) -> Result<Option<Vec<serde_json::Value>>> {
    let rpc_url = provider
        .rpc_config
        .qubitcoin_rpc_url
        .as_deref()
        .ok_or_else(|| {
            LabcoatError::new(
                "CONFIG_INVALID",
                "Qubitcoin mode is missing its RPC URL",
                "set rpc_url to the Qubitcoin endpoint",
            )
        })?;
    let encoded = JsonRpcProvider::call(
        provider,
        rpc_url,
        alkanes_cli_common::esplora::EsploraJsonRpcMethods::TX_HEX,
        serde_json::json!([txid]),
        1,
    )
    .await
    .map_err(|e| LabcoatError::classify(e.into()))?;
    let tx_hex = decode_qubitcoin_text(&encoded)?;
    let tx_bytes = hex::decode(&tx_hex).map_err(|e| {
        LabcoatError::new(
            "TOOLKIT_ERROR",
            format!("Qubitcoin returned invalid transaction hex: {e}"),
            "inspect the Qubitcoin Esplora indexer logs",
        )
    })?;
    let tx: bitcoin::Transaction = bitcoin::consensus::deserialize(&tx_bytes).map_err(|e| {
        LabcoatError::new(
            "TOOLKIT_ERROR",
            format!("failed to decode Qubitcoin transaction: {e}"),
            "inspect the Qubitcoin Esplora indexer logs",
        )
    })?;

    let decoded = alkanes_cli_common::runestone_enhanced::format_runestone_with_decoded_messages(
        &tx,
        provider.get_network(),
    )
    .map_err(|e| {
        LabcoatError::new(
            "TOOLKIT_ERROR",
            format!("failed to decode transaction runestone: {e}"),
            "re-run with RUST_LOG=debug",
        )
    })?;
    let count = decoded
        .get("protostones")
        .and_then(|p| p.as_array())
        .map_or(0, Vec::len);
    if count == 0 {
        return Ok(None);
    }

    let base_vout = tx.output.len() as u32 + 1;
    let mut traces = Vec::with_capacity(count);
    for index in 0..count {
        let outpoint = format!("{txid}:{}", base_vout + index as u32);
        let trace_pb = AlkanesProvider::trace(provider, &outpoint)
            .await
            .map_err(|e| LabcoatError::classify(e.into()))?;
        let Some(alkanes_trace) = trace_pb.trace else {
            traces.push(serde_json::json!({"events": []}));
            continue;
        };
        let trace_bytes = Message::encode_to_vec(&alkanes_trace);
        let support_trace = alkanes_support::proto::alkanes::AlkanesTrace::decode(
            trace_bytes.as_slice(),
        )
        .map_err(|e| {
            LabcoatError::new(
                "TOOLKIT_ERROR",
                format!("failed to decode Alkanes trace: {e}"),
                "the pinned toolkit and runtime indexer may be out of sync",
            )
        })?;
        let trace: alkanes_support::trace::Trace = support_trace.into();
        traces.push(alkanes_cli_common::alkanes::trace::trace_to_json(&trace));
    }

    Ok(Some(traces))
}

fn decode_qubitcoin_text(value: &serde_json::Value) -> Result<String> {
    let text = value.as_str().ok_or_else(|| {
        LabcoatError::new(
            "TOOLKIT_ERROR",
            format!("unexpected Qubitcoin secondary response: {value}"),
            "inspect the Qubitcoin Esplora indexer logs",
        )
    })?;
    let Some(encoded) = text.strip_prefix("0x") else {
        return Ok(text.to_string());
    };
    let bytes = hex::decode(encoded).map_err(|e| {
        LabcoatError::new(
            "TOOLKIT_ERROR",
            format!("Qubitcoin secondary response is not hex: {e}"),
            "inspect the Qubitcoin Esplora indexer logs",
        )
    })?;
    String::from_utf8(bytes).map_err(|e| {
        LabcoatError::new(
            "TOOLKIT_ERROR",
            format!("Qubitcoin secondary response is not UTF-8: {e}"),
            "inspect the Qubitcoin Esplora indexer logs",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_qubitcoin_hex_wrapped_text() {
        assert_eq!(
            decode_qubitcoin_text(&serde_json::json!("0x30323030")).unwrap(),
            "0200"
        );
        assert_eq!(
            decode_qubitcoin_text(&serde_json::json!("already-decoded")).unwrap(),
            "already-decoded"
        );
    }
}
