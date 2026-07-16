//! Transaction trace fetching + bounded wait.
//!
//! `trace_protostones` computes the protostone vouts itself
//! (tx.output.len() + 1 + i) — fixing the old TS `vout: 4` hardcode.

use crate::error::{LabcoatError, Result};
use alkanes_cli_common::provider::ConcreteProvider;
use alkanes_cli_common::traits::AlkanesProvider;

/// Fetch decoded traces for every protostone in a transaction.
/// Returns None when the tx carries no protostones.
pub async fn trace(
    provider: &ConcreteProvider,
    txid: &str,
) -> Result<Option<Vec<serde_json::Value>>> {
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
        match provider.trace_protostones(txid).await {
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
                format!("no trace for {} after {:?}", txid, timeout),
                "is metashrew synced? labcoat status; then labcoat trace <txid>",
            ));
        }
        tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
    }
}
