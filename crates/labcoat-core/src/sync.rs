//! Sync awareness: state-changing operations wait until the alkanes
//! indexer (metashrew) has caught up with the chain tip, so freshly mined
//! UTXOs are introspectable before we spend or trace against them.

use crate::error::{LabcoatError, Result};
use alkanes_cli_common::provider::ConcreteProvider;
use alkanes_cli_common::traits::{BitcoinRpcProvider, MetashrewRpcProvider};

/// Wait (bounded) for indexer height >= chain height.
/// Returns the indexed height to pass as `max_indexed_height`.
pub async fn wait_for_indexer(
    provider: &ConcreteProvider,
    timeout: std::time::Duration,
) -> Result<u64> {
    let started = std::time::Instant::now();
    loop {
        let chain = provider
            .get_block_count()
            .await
            .map_err(|e| LabcoatError::classify(e.into()))?;
        let indexed = provider
            .get_metashrew_height()
            .await
            .map_err(|e| LabcoatError::classify(e.into()))?;
        // metashrew reports the height it is *working on*; being one ahead
        // of the chain tip means fully synced.
        if indexed >= chain {
            return Ok(indexed);
        }
        if started.elapsed() > timeout {
            return Err(LabcoatError::new(
                "INDEXER_LAG",
                format!("indexer at {} but chain at {}", indexed, chain),
                "wait for metashrew to catch up (labcoat status / labcoat logs --service metashrew)",
            ));
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}
