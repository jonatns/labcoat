//! High-level toolkit operations — the exact functions the CLI, the MCP
//! server expose.

use crate::error::{LabcoatError, Result};
use crate::execute::{
    find_created_alkane, find_return_status, spec_with_options, ExecuteOutcome, TxOptions,
};
use crate::system::ToolkitConfig;
use crate::{lockfile, simulate as sim, sync, system, trace as trace_mod, wallet};
use std::path::Path;

const INDEXER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
const TRACE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// Where a deploy's wasm envelope lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DeployTarget {
    /// Next free id via the `[1,0]` factory target.
    #[default]
    New,
    /// Reserved number `N` via the `[3,N]` target.
    Reserve(u128),
}

impl DeployTarget {
    fn block_tx(self) -> (u128, u128) {
        match self {
            DeployTarget::New => (1, 0),
            DeployTarget::Reserve(n) => (3, n),
        }
    }
}

/// The deploy cellpack spec. `[1,0]` with no constructor inputs keeps its
/// historical bare form (matching the old encipher([1,0])); every other
/// shape carries initializer opcode 0 followed by the constructor args.
pub fn deploy_spec(target: DeployTarget, cellpack_args: &[u128], options: &TxOptions) -> String {
    let (block, tx) = target.block_tx();
    let inputs = if target == DeployTarget::New && cellpack_args.is_empty() {
        Vec::new()
    } else {
        let mut inputs = vec![0_u128];
        inputs.extend_from_slice(cellpack_args);
        inputs
    };
    spec_with_options(block, tx, &inputs, options)
}

/// Everything one deploy needs beyond config, signer, and project root.
pub struct DeployRequest<'a> {
    pub wasm_path: &'a Path,
    /// Lockfile name to record the deployment under (None skips recording).
    pub contract_name: Option<String>,
    pub cellpack_args: &'a [u128],
    pub fee_rate: Option<f32>,
    pub target: DeployTarget,
    pub options: &'a TxOptions,
}

/// Deploy a compiled contract (raw .wasm — the envelope gzips internally;
/// .wasm.gz inputs are rejected to prevent double compression).
pub async fn deploy(
    config: &ToolkitConfig,
    passphrase: Option<String>,
    request: DeployRequest<'_>,
) -> Result<ExecuteOutcome> {
    let deployment_root = std::env::current_dir().unwrap_or_else(|_| ".".into());
    deploy_in(config, passphrase, &deployment_root, request).await
}

/// Deploy and record the resulting contract in a specific project directory.
pub async fn deploy_in(
    config: &ToolkitConfig,
    passphrase: Option<String>,
    deployment_root: &Path,
    request: DeployRequest<'_>,
) -> Result<ExecuteOutcome> {
    let DeployRequest {
        wasm_path,
        contract_name,
        cellpack_args,
        fee_rate,
        target,
        options,
    } = request;
    if wasm_path.extension().and_then(|e| e.to_str()) == Some("gz") {
        return Err(LabcoatError::new(
            "ENVELOPE_INVALID",
            format!(
                "{} looks gzipped — deploy wants the raw .wasm (the reveal envelope compresses internally)",
                wasm_path.display()
            ),
            "pass the .wasm produced by `labcoat build`",
        ));
    }
    let wasm = std::fs::read(wasm_path).map_err(|e| {
        LabcoatError::new(
            "CONFIG_INVALID",
            format!("cannot read {}: {}", wasm_path.display(), e),
            "run `labcoat build` first",
        )
    })?;
    // A gzip magic check catches renamed files too.
    if wasm.starts_with(&[0x1f, 0x8b]) {
        return Err(LabcoatError::new(
            "ENVELOPE_INVALID",
            "wasm payload is gzip-compressed; deploy wants the raw .wasm".to_string(),
            "pass the .wasm produced by `labcoat build`",
        ));
    }

    config.require_passphrase_policy(&passphrase)?;
    let input_requirements = options.input_requirements()?;
    let mut provider = system::connect(config, passphrase, true).await?;
    let to_address = match &options.to {
        Some(address) => address.clone(),
        None => wallet::primary_address(&provider).await?,
    };
    let indexed = sync::wait_for_indexer(&provider, INDEXER_TIMEOUT)
        .await
        .ok();

    let spec = deploy_spec(target, cellpack_args, options);

    let result = crate::execute::run(
        &mut provider,
        config,
        crate::execute::ExecuteRequest {
            spec: &spec,
            envelope: Some(wasm.clone()),
            to_address,
            fee_rate,
            max_indexed_height: indexed,
            input_requirements,
        },
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
        use alkanes_cli_common::traits::BitcoinRpcProvider;
        use sha2::Digest;
        let network = config.network_id();
        // Chain instance identity: block 1's hash changes on every reset,
        // marking older records as stale. Best effort — a record without it
        // is still valid, just unverifiable.
        let chain_id = BitcoinRpcProvider::get_block_hash(&provider, 1).await.ok();
        lockfile::record(
            deployment_root,
            network,
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
                chain_id,
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

/// The call cellpack spec: opcode followed by args, with routing/edicts.
pub fn call_spec(
    block: u128,
    tx: u128,
    opcode: u128,
    args: &[u128],
    options: &TxOptions,
) -> String {
    let mut inputs = vec![opcode];
    inputs.extend_from_slice(args);
    spec_with_options(block, tx, &inputs, options)
}

/// Everything one call needs beyond config and signer.
pub struct CallRequest<'a> {
    pub block: u128,
    pub tx: u128,
    pub opcode: u128,
    pub args: &'a [u128],
    pub fee_rate: Option<f32>,
    pub options: &'a TxOptions,
}

/// Execute (state-changing call) against a deployed contract.
pub async fn call(
    config: &ToolkitConfig,
    passphrase: Option<String>,
    request: CallRequest<'_>,
) -> Result<ExecuteOutcome> {
    let CallRequest {
        block,
        tx,
        opcode,
        args,
        fee_rate,
        options,
    } = request;
    config.require_passphrase_policy(&passphrase)?;
    let input_requirements = options.input_requirements()?;
    let mut provider = system::connect(config, passphrase, true).await?;
    let to_address = match &options.to {
        Some(address) => address.clone(),
        None => wallet::primary_address(&provider).await?,
    };
    let indexed = sync::wait_for_indexer(&provider, INDEXER_TIMEOUT)
        .await
        .ok();

    let spec = call_spec(block, tx, opcode, args, options);
    let result = crate::execute::run(
        &mut provider,
        config,
        crate::execute::ExecuteRequest {
            spec: &spec,
            envelope: None,
            to_address,
            fee_rate,
            max_indexed_height: indexed,
            input_requirements,
        },
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

/// Alkanes token balances held by an address.
pub async fn balances(config: &ToolkitConfig, address: &str) -> Result<serde_json::Value> {
    use alkanes_cli_common::traits::AlkanesProvider;
    let provider = system::connect(config, None, false).await?;
    let balances = AlkanesProvider::get_balance(&provider, Some(address))
        .await
        .map_err(|e| LabcoatError::classify(e.into()))?;
    serde_json::to_value(balances).map_err(|e| {
        LabcoatError::new(
            "TOOLKIT_ERROR",
            format!("cannot serialize balances: {e}"),
            "report this as a Labcoat bug",
        )
    })
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
    let network = config.network_id();
    let dep = lockfile::get(&cwd, network, name)?.ok_or_else(|| {
        LabcoatError::new(
            "CONTRACT_NOT_FOUND",
            format!("no deployment of '{}' on {} in labcoat.lock", name, network),
            "deploy the contract first or pass its block:tx id directly",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deploy_spec_keeps_the_bare_historical_new_form() {
        let options = TxOptions::default();
        assert_eq!(deploy_spec(DeployTarget::New, &[], &options), "[1,0]:v0:v0");
        assert_eq!(
            deploy_spec(DeployTarget::New, &[3, 100], &options),
            "[1,0,0,3,100]:v0:v0"
        );
    }

    #[test]
    fn reserve_deploys_always_carry_the_initializer_opcode() {
        let options = TxOptions::default();
        assert_eq!(
            deploy_spec(DeployTarget::Reserve(65_011), &[], &options),
            "[3,65011,0]:v0:v0"
        );
        assert_eq!(
            deploy_spec(DeployTarget::Reserve(65_011), &[1, 100], &options),
            "[3,65011,0,1,100]:v0:v0"
        );
    }

    #[test]
    fn call_spec_carries_routing_options() {
        let options = TxOptions {
            pointer: Some("v1".into()),
            edicts: vec!["4:65014:100:v0".into()],
            ..TxOptions::default()
        };
        assert_eq!(
            call_spec(4, 65_014, 101, &[], &options),
            "[4,65014,101]:v1:v1:[4:65014:100:v0]"
        );
    }
}
