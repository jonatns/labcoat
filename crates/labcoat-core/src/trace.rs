//! Transaction trace fetching + bounded wait.
//!
//! `trace_protostones` computes protostone vouts from the transaction output
//! count instead of assuming a fixed index.

use crate::error::{LabcoatError, Result};
use alkanes_cli_common::provider::ConcreteProvider;
use alkanes_cli_common::traits::{BitcoinRpcProvider, JsonRpcProvider};
use alkanes_support::proto::alkanes::{alkanes_trace_event::Event, AlkanesTrace, Outpoint};
use alkanes_support::trace::{Trace, TraceEvent};
use bitcoin::hashes::Hash;
use prost::Message;

/// Fetch decoded traces for every protostone in a transaction.
/// Returns None when the tx carries no protostones.
pub async fn trace(
    provider: &ConcreteProvider,
    txid: &str,
) -> Result<Option<Vec<serde_json::Value>>> {
    let tx_hex = transaction_hex(provider, txid).await?;
    trace_transaction(provider, &tx_hex).await
}

async fn transaction_hex(provider: &ConcreteProvider, txid: &str) -> Result<String> {
    if provider.rpc_config.is_qubitcoin_mode() {
        qubitcoin_transaction_hex(provider, txid).await
    } else {
        provider
            .get_transaction_hex(txid)
            .await
            .map_err(|e| LabcoatError::classify(e.into()))
    }
}

/// Poll until traces exist for the tx (the indexer may lag the broadcast),
/// with a bounded timeout.
pub async fn wait_for_trace(
    provider: &ConcreteProvider,
    txid: &str,
    timeout: std::time::Duration,
) -> Result<Vec<serde_json::Value>> {
    wait_for_trace_every(provider, txid, None, timeout, REGTEST_POLL).await
}

/// How often a regtest target is asked for a trace: its block is already
/// mined, so the answer is at most an indexing pass away.
const REGTEST_POLL: std::time::Duration = std::time::Duration::from_millis(1000);

/// How often a public network is asked for a trace. The wait is for a
/// block, minutes away, and hosted endpoints count requests: Subfrost's
/// allows 20 a minute without an API key, which a one-second poll spends
/// in the first 20 seconds of a ten-minute wait.
pub const PUBLIC_POLL: std::time::Duration = std::time::Duration::from_secs(10);

/// [`wait_for_trace`] at a chosen pace. With the transaction's hex in hand
/// (the caller just broadcast it) each poll is one request per protostone
/// and the transaction is never fetched back from the node.
pub async fn wait_for_trace_every(
    provider: &ConcreteProvider,
    txid: &str,
    tx_hex: Option<&str>,
    timeout: std::time::Duration,
    interval: std::time::Duration,
) -> Result<Vec<serde_json::Value>> {
    let started = std::time::Instant::now();
    // Fetched at most once: a transaction's bytes do not change.
    let mut tx_hex = tx_hex.map(str::to_owned);
    loop {
        let polled = match &tx_hex {
            Some(hex) => trace_transaction(provider, hex).await,
            None => match transaction_hex(provider, txid).await {
                Ok(hex) => {
                    let traced = trace_transaction(provider, &hex).await;
                    tx_hex = Some(hex);
                    traced
                }
                Err(error) => Err(error),
            },
        };
        match polled {
            Ok(Some(traces)) if !traces.is_empty() => {
                let has_events = traces.iter().any(|trace| {
                    let events = trace
                        .get("trace")
                        .or_else(|| trace.get("events"))
                        .unwrap_or(trace);
                    match events {
                        serde_json::Value::Array(events) => !events.is_empty(),
                        serde_json::Value::Object(event) => {
                            event.contains_key("type") || event.contains_key("event")
                        }
                        _ => false,
                    }
                });
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
        tokio::time::sleep(interval).await;
    }
}

/// Read the transaction through the node's Esplora view, as the Alkanes
/// client does for Qubitcoin-protocol nodes.
async fn qubitcoin_transaction_hex(provider: &ConcreteProvider, txid: &str) -> Result<String> {
    let rpc_url = provider
        .rpc_config
        .qubitcoin_rpc_url
        .as_deref()
        .ok_or_else(|| {
            LabcoatError::new(
                "CONFIG_INVALID",
                "the network has no RPC URL",
                "set rpc_url to the network's JSON-RPC endpoint",
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
    decode_qubitcoin_text(&encoded)
}

/// Query Alkanes traces for the virtual protostone outpoints of a
/// transaction. Protostone `i` is traced at vout `outputs + 1 + i`.
async fn trace_transaction(
    provider: &ConcreteProvider,
    tx_hex: &str,
) -> Result<Option<Vec<serde_json::Value>>> {
    let tx_bytes = hex::decode(tx_hex).map_err(|e| {
        LabcoatError::new(
            "TOOLKIT_ERROR",
            format!("the node returned invalid transaction hex: {e}"),
            "run `labcoat logs`",
        )
    })?;
    let tx: bitcoin::Transaction = bitcoin::consensus::deserialize(&tx_bytes).map_err(|e| {
        LabcoatError::new(
            "TOOLKIT_ERROR",
            format!("failed to decode the node's transaction: {e}"),
            "run `labcoat logs`",
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

    // The upstream client decodes a trace with a protobuf type that predates
    // `fuel_used` and drops it, so the view is called and decoded here.
    let txid_bytes = tx.compute_txid().to_byte_array().to_vec();
    let base_vout = tx.output.len() as u32 + 1;
    let mut traces = Vec::with_capacity(count);
    for index in 0..count {
        let outpoint = Outpoint {
            txid: txid_bytes.clone(),
            vout: base_vout + index as u32,
        };
        let request = format!("0x{}", hex::encode(outpoint.encode_to_vec()));
        let served = provider
            .metashrew_view_call("trace", &request, "latest")
            .await
            .map_err(|e| LabcoatError::classify(e.into()))?;
        if served.is_empty() {
            traces.push(serde_json::json!({"events": []}));
            continue;
        }
        let indexed = AlkanesTrace::decode(served.as_slice()).map_err(|e| {
            LabcoatError::new(
                "TOOLKIT_ERROR",
                format!("failed to decode Alkanes trace: {e}"),
                "the pinned toolkit and runtime indexer may be out of sync",
            )
        })?;
        traces.push(trace_json(indexed));
    }

    Ok(Some(traces))
}

/// Convert an indexer trace to JSON, keeping the fuel each call frame used.
///
/// The pinned alkanes-support also converts an exit event from its response
/// alone, which leaves `fuel_used` at zero. Events convert one to one and in
/// order, so the fuel is copied across by position.
fn trace_json(indexed: AlkanesTrace) -> serde_json::Value {
    let fuel: Vec<Option<u64>> = indexed
        .events
        .iter()
        .map(|event| match &event.event {
            Some(Event::ExitContext(exit)) => Some(exit.fuel_used),
            _ => None,
        })
        .collect();
    let trace: Trace = indexed.into();
    for (event, fuel) in trace.0.lock().unwrap().iter_mut().zip(fuel) {
        if let (
            TraceEvent::ReturnContext(response) | TraceEvent::RevertContext(response),
            Some(fuel),
        ) = (event, fuel)
        {
            response.fuel_used = fuel;
        }
    }
    alkanes_cli_common::alkanes::trace::trace_to_json(&trace)
}

fn decode_qubitcoin_text(value: &serde_json::Value) -> Result<String> {
    let text = value.as_str().ok_or_else(|| {
        LabcoatError::new(
            "TOOLKIT_ERROR",
            format!("unexpected indexer response: {value}"),
            "run `labcoat logs`",
        )
    })?;
    let Some(encoded) = text.strip_prefix("0x") else {
        return Ok(text.to_string());
    };
    let bytes = hex::decode(encoded).map_err(|e| {
        LabcoatError::new(
            "TOOLKIT_ERROR",
            format!("the indexer response is not hex: {e}"),
            "run `labcoat logs`",
        )
    })?;
    String::from_utf8(bytes).map_err(|e| {
        LabcoatError::new(
            "TOOLKIT_ERROR",
            format!("the indexer response is not UTF-8: {e}"),
            "run `labcoat logs`",
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

    #[test]
    fn keeps_the_fuel_the_indexer_recorded_for_each_frame() {
        use alkanes_support::proto::alkanes::{
            AlkanesEnterContext, AlkanesExitContext, AlkanesTraceEvent, ExtendedCallResponse,
        };
        let enter = || AlkanesTraceEvent {
            event: Some(Event::EnterContext(AlkanesEnterContext::default())),
        };
        let exit = |status: i32, fuel_used: u64| AlkanesTraceEvent {
            event: Some(Event::ExitContext(AlkanesExitContext {
                status,
                response: Some(ExtendedCallResponse::default()),
                fuel_used,
            })),
        };
        // An outer call that makes an inner call which reverts.
        let indexed = AlkanesTrace {
            events: vec![enter(), enter(), exit(1, 4_200), exit(0, 123_479)],
        };

        let json = trace_json(indexed);
        let events = json["trace"].as_array().unwrap();
        assert_eq!(events[2]["type"], "revert");
        assert_eq!(events[2]["fuel_used"], 4_200);
        assert_eq!(events[3]["type"], "return");
        assert_eq!(events[3]["fuel_used"], 123_479);
    }

    #[test]
    fn decodes_fuel_from_a_trace_the_indexer_served() {
        // A call to a contract that returned sixteen zero bytes.
        let served = hex::decode(
            "0a3d0a3b080112370a300a080a0208021202080412040a0012001a0208011a0208011a001a001a001a\
             001a001a001a001a001a001a001a00200410e0cfd5010a1a121812121a100000000000000000000000\
             000000000018d7c407",
        )
        .unwrap();
        let json = trace_json(AlkanesTrace::decode(served.as_slice()).unwrap());
        let events = json["trace"].as_array().unwrap();
        assert_eq!(events[0]["fuel_allocated"], 3_500_000);
        assert_eq!(events[1]["type"], "return");
        assert_eq!(events[1]["fuel_used"], 123_479);
    }
}
