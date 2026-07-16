//! High-level toolkit operations — the exact functions the CLI, the MCP
//! server expose.

use crate::error::{LabcoatError, Result};
use crate::execute::{cellpack_spec, find_created_alkane, find_return_status, ExecuteOutcome};
use crate::system::ToolkitConfig;
use crate::{lockfile, simulate as sim, sync, system, trace as trace_mod, wallet};
use std::path::Path;

const INDEXER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
const TRACE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// Deploy a compiled contract (raw .wasm — the envelope gzips internally;
/// .wasm.gz inputs are rejected to prevent double compression).
pub async fn deploy(
    config: &ToolkitConfig,
    passphrase: Option<String>,
    wasm_path: &Path,
    contract_name: Option<String>,
    cellpack_args: &[u128],
    fee_rate: Option<f32>,
) -> Result<ExecuteOutcome> {
    if wasm_path.extension().and_then(|e| e.to_str()) == Some("gz") {
        return Err(LabcoatError::new(
            "ENVELOPE_INVALID",
            format!(
                "{} looks gzipped — deploy wants the raw .wasm (the reveal envelope compresses internally)",
                wasm_path.display()
            ),
            "pass the .wasm produced by `labcoat compile`",
        ));
    }
    let wasm = std::fs::read(wasm_path).map_err(|e| {
        LabcoatError::new(
            "CONFIG_INVALID",
            format!("cannot read {}: {}", wasm_path.display(), e),
            "run `labcoat compile` first",
        )
    })?;
    // A gzip magic check catches renamed files too.
    if wasm.starts_with(&[0x1f, 0x8b]) {
        return Err(LabcoatError::new(
            "ENVELOPE_INVALID",
            "wasm payload is gzip-compressed; deploy wants the raw .wasm".to_string(),
            "pass the .wasm produced by `labcoat compile`",
        ));
    }

    config.require_passphrase_policy(&passphrase)?;
    let mut provider = system::connect(config, passphrase, true).await?;
    let to_address = wallet::primary_address(&provider).await?;
    let indexed = sync::wait_for_indexer(&provider, INDEXER_TIMEOUT)
        .await
        .ok();

    // Deploy-new cellpack target is 1:0.
    let spec = cellpack_spec(1, 0, 0, cellpack_args);
    // The [1,0,...] form: block=1, tx=0, then constructor args — opcode 0 is
    // part of the constructor input stream, matching the old encipher([1n,0n]).
    let spec = if cellpack_args.is_empty() {
        "[1,0]:v0:v0".to_string()
    } else {
        spec
    };

    let result = crate::execute::run(
        &mut provider,
        config,
        &spec,
        Some(wasm.clone()),
        to_address,
        fee_rate,
        indexed,
    )
    .await?;

    // Prefer the traces attached by execute_full; fall back to polling.
    let traces = match &result.traces {
        Some(t) if !t.is_empty() => Some(t.clone()),
        _ => trace_mod::wait_for_trace(&provider, &result.reveal_txid, TRACE_TIMEOUT)
            .await
            .ok(),
    };

    let alkanes_id = find_created_alkane(&traces);
    let (status, revert_reason) = find_return_status(&traces);

    if let (Some(id), Some(name)) = (&alkanes_id, &contract_name) {
        use sha2::Digest;
        let network = config.normalized_network();
        let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
        lockfile::record(
            &cwd,
            &network,
            name,
            lockfile::Deployment {
                alkanes_id: id.clone(),
                wasm_sha256: Some(hex::encode(sha2::Sha256::digest(&wasm))),
                txid: result.reveal_txid.clone(),
                block: None,
                status: status.clone(),
                deployed_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64,
            },
        )?;
    }

    Ok(ExecuteOutcome {
        commit_txid: result.commit_txid,
        txid: result.reveal_txid,
        commit_fee: result.commit_fee,
        fee: result.reveal_fee,
        status,
        revert_reason,
        alkanes_id,
        traces,
    })
}

/// Execute (state-changing call) against a deployed contract.
pub async fn call(
    config: &ToolkitConfig,
    passphrase: Option<String>,
    block: u128,
    tx: u128,
    opcode: u128,
    args: &[u128],
    fee_rate: Option<f32>,
) -> Result<ExecuteOutcome> {
    config.require_passphrase_policy(&passphrase)?;
    let mut provider = system::connect(config, passphrase, true).await?;
    let to_address = wallet::primary_address(&provider).await?;
    let indexed = sync::wait_for_indexer(&provider, INDEXER_TIMEOUT)
        .await
        .ok();

    let spec = cellpack_spec(block, tx, opcode, args);
    let result = crate::execute::run(
        &mut provider,
        config,
        &spec,
        None,
        to_address,
        fee_rate,
        indexed,
    )
    .await?;

    let traces = match &result.traces {
        Some(t) if !t.is_empty() => Some(t.clone()),
        _ => trace_mod::wait_for_trace(&provider, &result.reveal_txid, TRACE_TIMEOUT)
            .await
            .ok(),
    };
    let (status, revert_reason) = find_return_status(&traces);

    Ok(ExecuteOutcome {
        commit_txid: result.commit_txid,
        txid: result.reveal_txid,
        commit_fee: result.commit_fee,
        fee: result.reveal_fee,
        status,
        revert_reason,
        alkanes_id: None,
        traces,
    })
}

/// Read-only simulation.
pub async fn simulate(
    config: &ToolkitConfig,
    block: u128,
    tx: u128,
    opcode: u128,
    args: &[u128],
) -> Result<sim::SimulateOutcome> {
    let provider = system::connect(config, None, false).await?;
    sim::simulate(&provider, block, tx, opcode, args).await
}

/// Decoded traces for a txid (optionally waiting for the indexer).
pub async fn trace(
    config: &ToolkitConfig,
    txid: &str,
    wait: bool,
) -> Result<Option<Vec<serde_json::Value>>> {
    let provider = system::connect(config, None, false).await?;
    if wait {
        trace_mod::wait_for_trace(&provider, txid, TRACE_TIMEOUT)
            .await
            .map(Some)
    } else {
        trace_mod::trace(&provider, txid).await
    }
}

/// Resolve a contract's alkanes id from the lockfile.
pub fn resolve_contract(config: &ToolkitConfig, name: &str) -> Result<(u128, u128)> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let network = config.normalized_network();
    let dep = lockfile::get(&cwd, &network, name).ok_or_else(|| {
        LabcoatError::new(
            "CONTRACT_NOT_FOUND",
            format!("no deployment of '{}' on {} in labcoat.lock", name, network),
            "deploy it first, or run `labcoat lock migrate` for legacy manifests",
        )
    })?;
    parse_alkanes_id(&dep.alkanes_id)
}

pub fn parse_alkanes_id(id: &str) -> Result<(u128, u128)> {
    let mut parts = id.split(':');
    let (Some(b), Some(t)) = (parts.next(), parts.next()) else {
        return Err(LabcoatError::new(
            "CONFIG_INVALID",
            format!("bad alkanes id '{}'", id),
            "expected block:tx",
        ));
    };
    let block = b.trim().parse().map_err(|_| {
        LabcoatError::new(
            "CONFIG_INVALID",
            format!("bad block in '{}'", id),
            "expected block:tx",
        )
    })?;
    let tx = t.trim().parse().map_err(|_| {
        LabcoatError::new(
            "CONFIG_INVALID",
            format!("bad tx in '{}'", id),
            "expected block:tx",
        )
    })?;
    Ok((block, tx))
}
