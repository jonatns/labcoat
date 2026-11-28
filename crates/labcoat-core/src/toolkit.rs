//! High-level toolkit operations — the exact functions the CLI, the MCP
//! server expose.

use crate::error::{LabcoatError, Result};
use crate::execute::{
    find_created_alkane, find_created_alkanes, find_return_status, spec_with_options,
    BroadcastObserver, ExecuteOutcome, TxOptions,
};
use crate::signer::SignerSpec;
use crate::system::ToolkitConfig;
use crate::{lockfile, simulate as sim, sync, system, trace as trace_mod, wallet};
use alkanes_cli_common::alkanes::types::{EnhancedExecuteResult, ReadyToSignRevealTx};
use alkanes_cli_common::provider::ConcreteProvider;
use alkanes_cli_common::traits::BitcoinRpcProvider;
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
    /// A new instance of an already-deployed template's code, via the
    /// factory target `[factory_block, tx]`. Build with
    /// [`DeployTarget::clone_of`].
    Clone { factory_block: u128, tx: u128 },
}

impl DeployTarget {
    fn block_tx(self) -> (u128, u128) {
        match self {
            DeployTarget::New => (1, 0),
            DeployTarget::Reserve(n) => (3, n),
            DeployTarget::Clone { factory_block, tx } => (factory_block, tx),
        }
    }

    /// The factory target that clones the template at `block:tx`: `[5,tx]`
    /// for a sequential `2:tx` template, `[6,tx]` for a reserved `4:tx`
    /// one. No other block holds clonable code.
    pub fn clone_of(block: u128, tx: u128) -> Result<Self> {
        let factory_block = match block {
            2 => 5,
            4 => 6,
            other => {
                return Err(LabcoatError::new(
                    "CONFIG_INVALID",
                    format!(
                    "cannot clone {other}:{tx} — only 2:n and 4:n Alkanes are clonable templates"
                ),
                    "clone a contract deployed with a sequential (2:n) or reserved (4:n) id",
                ))
            }
        };
        Ok(DeployTarget::Clone { factory_block, tx })
    }

    /// The cellpack target, for display: `1:0`, `3:N`, `5:N`, or `6:N`.
    pub fn label(self) -> String {
        let (block, tx) = self.block_tx();
        format!("{block}:{tx}")
    }
}

/// The deploy cellpack spec. `[1,0]` with no constructor inputs keeps its
/// historical bare form (matching the old encipher([1,0])); every other
/// shape carries initializer opcode 0 followed by the constructor args.
pub fn deploy_spec(target: DeployTarget, cellpack_args: &[u128], options: &TxOptions) -> String {
    deploy_spec_with(target, None, cellpack_args, options)
}

/// The deploy cellpack spec with an explicit initializer opcode. `None`
/// keeps [`deploy_spec`]'s behavior (opcode 0, or the bare `[1,0]` form);
/// `Some(opcode)` always carries that opcode followed by the args, which is
/// how standard proxies (`0x7fff`) and uninitialized templates deploy.
pub fn deploy_spec_with(
    target: DeployTarget,
    initializer: Option<u128>,
    cellpack_args: &[u128],
    options: &TxOptions,
) -> String {
    let (block, tx) = target.block_tx();
    let inputs = match initializer {
        None if target == DeployTarget::New && cellpack_args.is_empty() => Vec::new(),
        opcode => {
            let mut inputs = vec![opcode.unwrap_or(0)];
            inputs.extend_from_slice(cellpack_args);
            inputs
        }
    };
    spec_with_options(block, tx, &inputs, options)
}

/// Everything one deploy needs beyond config, signer, and project root.
pub struct DeployRequest<'a> {
    /// The raw `.wasm` to deploy. Ignored for [`DeployTarget::Clone`],
    /// which reuses the template's code and carries no envelope.
    pub wasm_path: &'a Path,
    /// Lockfile name to record the deployment under (None skips recording).
    pub contract_name: Option<String>,
    /// Initializer opcode; `None` is the opcode-0 constructor.
    pub initializer: Option<u128>,
    pub cellpack_args: &'a [u128],
    pub fee_rate: Option<f32>,
    pub target: DeployTarget,
    pub options: &'a TxOptions,
}

/// Deploy a compiled contract (raw .wasm — the envelope gzips internally;
/// .wasm.gz inputs are rejected to prevent double compression).
pub async fn deploy(
    config: &ToolkitConfig,
    signer: &SignerSpec,
    request: DeployRequest<'_>,
) -> Result<ExecuteOutcome> {
    let deployment_root = config.project_root();
    deploy_in(config, signer, &deployment_root, request).await
}

/// Deploy and record the resulting contract in a specific project directory.
pub async fn deploy_in(
    config: &ToolkitConfig,
    signer: &SignerSpec,
    deployment_root: &Path,
    request: DeployRequest<'_>,
) -> Result<ExecuteOutcome> {
    deploy_with_broadcast(config, signer, deployment_root, request, None).await
}

pub(crate) async fn deploy_with_broadcast(
    config: &ToolkitConfig,
    signer: &SignerSpec,
    deployment_root: &Path,
    request: DeployRequest<'_>,
    observe: Option<&mut BroadcastObserver<'_>>,
) -> Result<ExecuteOutcome> {
    let DeployRequest {
        wasm_path,
        contract_name,
        initializer,
        cellpack_args,
        fee_rate,
        target,
        options,
    } = request;
    let is_clone = matches!(target, DeployTarget::Clone { .. });
    if !is_clone && wasm_path.extension().and_then(|e| e.to_str()) == Some("gz") {
        return Err(LabcoatError::new(
            "ENVELOPE_INVALID",
            format!(
                "{} looks gzipped — deploy wants the raw .wasm (the reveal envelope compresses internally)",
                wasm_path.display()
            ),
            "pass the .wasm produced by `labcoat build`",
        ));
    }
    let wasm = if is_clone {
        None
    } else {
        Some(std::fs::read(wasm_path).map_err(|e| {
            LabcoatError::new(
                "CONFIG_INVALID",
                format!("cannot read {}: {}", wasm_path.display(), e),
                "run `labcoat build` first",
            )
        })?)
    };
    // A gzip magic check catches renamed files too.
    if wasm
        .as_ref()
        .is_some_and(|wasm| wasm.starts_with(&[0x1f, 0x8b]))
    {
        return Err(LabcoatError::new(
            "ENVELOPE_INVALID",
            "wasm payload is gzip-compressed; deploy wants the raw .wasm".to_string(),
            "pass the .wasm produced by `labcoat build`",
        ));
    }

    if !options.payments.is_empty() {
        return Err(crate::execute::payments_on_deploy());
    }
    let input_requirements = options.input_requirements()?;
    let mut provider = system::connect_signing(config, signer).await?.provider;
    let to_address = match &options.to {
        Some(address) => address.clone(),
        None => wallet::primary_address(&provider).await?,
    };
    let indexed = sync::wait_for_indexer(&provider, INDEXER_TIMEOUT)
        .await
        .ok();

    // Chain instance identity: block 1's hash changes on every reset,
    // marking older records as stale. Best effort — a record without it
    // is still valid, just unverifiable.
    let chain_id = BitcoinRpcProvider::get_block_hash(&provider, 1).await.ok();
    // Durable-state guard: when version-2 state exists for this
    // environment, a reset or foreign chain aborts here — before any
    // broadcast — and the environment lease is held across the deploy.
    let mut state_guard = crate::state::deploy_guard(
        deployment_root,
        &config.environment,
        &crate::state::observed_chain(config, chain_id.clone()),
    )?;

    let spec = deploy_spec_with(target, initializer, cellpack_args, options);

    let result = crate::execute::run_with_broadcast(
        &mut provider,
        config,
        crate::execute::ExecuteRequest {
            spec: &spec,
            envelope: wasm.clone(),
            to_address,
            fee_rate,
            max_indexed_height: indexed,
            input_requirements,
            payments: Vec::new(),
        },
        observe,
    )
    .await?;
    let outcome = execution_outcome(&provider, config, result, true).await;

    if let (Some(id), Some(name)) = (&outcome.alkanes_id, &contract_name) {
        use sha2::Digest;
        let network = config.network_id();
        let deployment = lockfile::Deployment {
            alkanes_id: id.clone(),
            wasm_sha256: wasm
                .as_ref()
                .map(|wasm| hex::encode(sha2::Sha256::digest(wasm))),
            txid: outcome.txid.clone(),
            block: None,
            status: outcome.status.clone(),
            deployed_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            chain_id,
        };
        lockfile::record(deployment_root, network, name, deployment.clone())?;
        if let Some((mut lease, v2_state)) = state_guard.take() {
            if let Err(e) = crate::state::record_instance(
                &mut lease,
                v2_state,
                name,
                &deployment,
                crate::state::InstanceOrigin::Deployed,
                crate::state::InstanceExtras {
                    commit_txid: outcome.commit_txid.clone(),
                    labcoat_version: Some(env!("CARGO_PKG_VERSION").to_string()),
                    revert_reason: outcome.revert_reason.clone(),
                },
            ) {
                // labcoat.lock stays canonical in Milestone 1; failing a
                // broadcast deployment over the history mirror would lose
                // more than it protects.
                tracing::warn!("durable state not updated: {e}");
            }
        }
    }

    Ok(outcome)
}

pub(crate) async fn resume_deploy(
    config: &ToolkitConfig,
    signer: &SignerSpec,
    deployment_root: &Path,
    reveal: ReadyToSignRevealTx,
    observe: &mut BroadcastObserver<'_>,
) -> Result<ExecuteOutcome> {
    let mut provider = system::connect_signing(config, signer).await?.provider;
    let chain_id = BitcoinRpcProvider::get_block_hash(&provider, 1).await.ok();
    let _state_guard = crate::state::deploy_guard(
        deployment_root,
        &config.environment,
        &crate::state::observed_chain(config, chain_id),
    )?;
    let result = crate::execute::resume_reveal(&mut provider, config, reveal, observe).await?;
    Ok(execution_outcome(&provider, config, result, true).await)
}

/// Wait for a broadcast transaction's trace on a public network, where
/// that means waiting for a block: paced for a hosted endpoint, for as long
/// as the configuration allows, and reported as progress. `None` when the
/// wait ends with the transaction still unconfirmed.
async fn confirmed_trace(
    provider: &ConcreteProvider,
    config: &ToolkitConfig,
    txid: &str,
    tx_hex: Option<&str>,
) -> Option<Vec<serde_json::Value>> {
    use crate::progress::{report, Event};
    let (stop_waiting, stopped) = tokio::sync::oneshot::channel();
    report(config, Event::Waiting { txid, stop_waiting });
    let traces = tokio::select! {
        result = trace_mod::wait_for_trace_every(
            provider,
            txid,
            tx_hex,
            config.confirm_timeout(),
            trace_mod::PUBLIC_POLL,
        ) => result.ok(),
        Ok(()) = stopped => None,
    };
    report(
        config,
        Event::WaitEnded {
            txid,
            confirmed: traces.is_some(),
        },
    );
    traces
}

/// Wait for a transaction an earlier deploy sent to a public network.
/// `false` when it is still unconfirmed after the configured wait.
pub(crate) async fn await_confirmation(config: &ToolkitConfig, txid: &str) -> Result<bool> {
    let provider = system::connect(config, None, false).await?;
    Ok(confirmed_trace(&provider, config, txid, None)
        .await
        .is_some())
}

async fn execution_outcome(
    provider: &ConcreteProvider,
    config: &ToolkitConfig,
    result: EnhancedExecuteResult,
    deploy: bool,
) -> ExecuteOutcome {
    let public = !config.network.uses_regtest();
    let traces = match result.traces {
        Some(traces) if !traces.is_empty() => Some(traces),
        _ if public => {
            confirmed_trace(
                provider,
                config,
                &result.reveal_txid,
                result.reveal_tx_hex.as_deref(),
            )
            .await
        }
        _ => trace_mod::wait_for_trace(provider, &result.reveal_txid, TRACE_TIMEOUT)
            .await
            .ok(),
    };
    let (status, revert_reason) = find_return_status(&traces);
    ExecuteOutcome {
        confirmation: (public && traces.is_none()).then_some("pending"),
        commit_txid: result.commit_txid,
        txid: result.reveal_txid,
        commit_fee: result.commit_fee,
        fee: result.reveal_fee,
        status,
        revert_reason,
        alkanes_id: if deploy {
            find_created_alkane(&traces)
        } else {
            None
        },
        created: if deploy {
            find_created_alkanes(&traces)
        } else {
            Vec::new()
        },
        payments: Vec::new(),
        traces,
    }
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
    signer: &SignerSpec,
    request: CallRequest<'_>,
) -> Result<ExecuteOutcome> {
    call_with_broadcast(config, signer, request, None).await
}

pub(crate) async fn call_with_broadcast(
    config: &ToolkitConfig,
    signer: &SignerSpec,
    request: CallRequest<'_>,
    observe: Option<&mut BroadcastObserver<'_>>,
) -> Result<ExecuteOutcome> {
    let CallRequest {
        block,
        tx,
        opcode,
        args,
        fee_rate,
        options,
    } = request;
    let input_requirements = options.input_requirements()?;
    // Before connecting: a bad payment fails without touching the wallet.
    let spec = call_spec(block, tx, opcode, args, options);
    let payments =
        crate::execute::payment_outputs(&spec, options, crate::execute::bitcoin_network(config))?;
    let mut provider = system::connect_signing(config, signer).await?.provider;
    let to_address = match &options.to {
        Some(address) => address.clone(),
        None => wallet::primary_address(&provider).await?,
    };
    let indexed = sync::wait_for_indexer(&provider, INDEXER_TIMEOUT)
        .await
        .ok();

    let result = crate::execute::run_with_broadcast(
        &mut provider,
        config,
        crate::execute::ExecuteRequest {
            spec: &spec,
            envelope: None,
            to_address,
            fee_rate,
            max_indexed_height: indexed,
            input_requirements,
            payments: payments.clone(),
        },
        observe,
    )
    .await?;
    let mut outcome = execution_outcome(&provider, config, result, false).await;
    outcome.payments = payments;
    Ok(outcome)
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
    if wait && !config.network.uses_regtest() {
        // The transaction may still be waiting for a block.
        trace_mod::wait_for_trace_every(
            &provider,
            txid,
            None,
            config.confirm_timeout(),
            trace_mod::PUBLIC_POLL,
        )
        .await
        .map(Some)
    } else if wait {
        trace_mod::wait_for_trace(&provider, txid, TRACE_TIMEOUT)
            .await
            .map(Some)
    } else {
        trace_mod::trace(&provider, txid).await
    }
}

pub(crate) async fn chain_height(config: &ToolkitConfig) -> Result<u64> {
    let provider = system::connect(config, None, false).await?;
    BitcoinRpcProvider::get_block_count(&provider)
        .await
        .map_err(|error| LabcoatError::classify(error.into()))
}

/// Chain instance identity: block 1's hash (block 0 is reset-invariant on
/// regtest). Best-effort — `None` when the node is unreachable.
pub async fn chain_instance_id(config: &ToolkitConfig) -> Option<String> {
    let provider = system::connect(config, None, false).await.ok()?;
    BitcoinRpcProvider::get_block_hash(&provider, 1).await.ok()
}

/// Resolve a contract's alkanes id from the lockfile.
pub fn resolve_contract(config: &ToolkitConfig, name: &str) -> Result<(u128, u128)> {
    let network = config.network_id();
    let dep = lockfile::get(&config.project_root(), network, name)?.ok_or_else(|| {
        LabcoatError::new(
            "CONTRACT_NOT_FOUND",
            format!("no deployment of '{name}' on {network} in labcoat.lock"),
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
            format!("bad alkanes id '{id}'"),
            "expected block:tx",
        ));
    };
    let block = b.trim().parse().map_err(|_| {
        LabcoatError::new(
            "CONFIG_INVALID",
            format!("bad block in '{id}'"),
            "expected block:tx",
        )
    })?;
    let tx = t.trim().parse().map_err(|_| {
        LabcoatError::new(
            "CONFIG_INVALID",
            format!("bad tx in '{id}'"),
            "expected block:tx",
        )
    })?;
    Ok((block, tx))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A wallet-less provider for an endpoint nothing listens on: the
    /// transaction below carries no protostone, so no request is made.
    async fn offline(network: crate::NetworkTarget) -> (ToolkitConfig, ConcreteProvider) {
        let config = ToolkitConfig {
            network,
            rpc_url: "http://127.0.0.1:9".to_string(),
            wallet_file: std::env::temp_dir().join(format!(
                "labcoat-toolkit-wait-{}/wallet.json",
                std::process::id()
            )),
            confirm_timeout: Some(std::time::Duration::from_millis(25)),
            ..ToolkitConfig::default()
        };
        let provider = system::connect(&config, None, false).await.unwrap();
        (config, provider)
    }

    fn broadcast_result() -> EnhancedExecuteResult {
        let tx = bitcoin::Transaction {
            version: bitcoin::transaction::Version::TWO,
            lock_time: bitcoin::absolute::LockTime::ZERO,
            input: vec![bitcoin::TxIn::default()],
            output: vec![bitcoin::TxOut {
                value: bitcoin::Amount::from_sat(546),
                script_pubkey: bitcoin::ScriptBuf::new(),
            }],
        };
        EnhancedExecuteResult {
            commit_txid: None,
            commit_fee: None,
            reveal_txid: tx.compute_txid().to_string(),
            reveal_fee: 300,
            inputs_used: Vec::new(),
            outputs_created: Vec::new(),
            traces: None,
            split_txid: None,
            split_fee: None,
            wrap_txid: None,
            wrap_fee: None,
            reveal_tx_hex: Some(bitcoin::consensus::encode::serialize_hex(&tx)),
        }
    }

    #[tokio::test]
    async fn a_public_network_wait_that_runs_out_is_a_pending_outcome() {
        use std::sync::atomic::{AtomicBool, Ordering};

        static EVENTS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
        static INTERRUPT: AtomicBool = AtomicBool::new(false);
        crate::progress::set_reporter(|event| {
            EVENTS.lock().unwrap().push(match event {
                crate::progress::Event::Broadcast { kind, .. } => format!("broadcast {kind}"),
                crate::progress::Event::Waiting { stop_waiting, .. } => {
                    if INTERRUPT.load(Ordering::Relaxed) {
                        stop_waiting.send(()).unwrap();
                    }
                    "waiting".to_string()
                }
                crate::progress::Event::WaitEnded { confirmed, .. } => {
                    format!("ended confirmed={confirmed}")
                }
            });
        });

        let (mut config, provider) = offline(crate::NetworkTarget::Signet).await;
        let result = broadcast_result();
        let txid = result.reveal_txid.clone();
        let quickly = std::time::Duration::from_secs(1);
        let outcome = tokio::time::timeout(
            quickly,
            execution_outcome(&provider, &config, result, false),
        )
        .await
        .expect("the configured timeout is shorter than the poll interval");
        // Sent, not confirmed, and not a failure: the transaction id is
        // kept and the state is named.
        assert_eq!(outcome.txid, txid);
        assert_eq!(outcome.confirmation, Some("pending"));
        assert_ne!(outcome.status, "success");
        assert_ne!(outcome.status, "revert");
        assert!(outcome.traces.is_none());
        let json = serde_json::to_value(&outcome).unwrap();
        assert_eq!(json["confirmation"], "pending");
        // The wait is announced and its end reported, in that order.
        assert_eq!(
            *EVENTS.lock().unwrap(),
            ["waiting", "ended confirmed=false"]
        );

        // Nothing is reported for a regtest target, whatever it does.
        let local = ToolkitConfig::default();
        crate::progress::report(
            &local,
            crate::progress::Event::Broadcast {
                kind: "transaction",
                txid: &txid,
            },
        );
        assert_eq!(EVENTS.lock().unwrap().len(), 2);
        // On a public network a broadcast is.
        crate::progress::report(
            &config,
            crate::progress::Event::Broadcast {
                kind: "commit",
                txid: &txid,
            },
        );
        assert_eq!(EVENTS.lock().unwrap().last().unwrap(), "broadcast commit");

        INTERRUPT.store(true, Ordering::Relaxed);
        config.confirm_timeout = Some(trace_mod::PUBLIC_POLL);
        let result = broadcast_result();
        let interrupted = tokio::time::timeout(
            quickly,
            execution_outcome(&provider, &config, result, false),
        )
        .await
        .expect("the interrupt stops waiting before the timeout");
        assert_eq!(interrupted.confirmation, Some("pending"));
        assert_eq!(interrupted.txid, txid);
        assert!(EVENTS
            .lock()
            .unwrap()
            .ends_with(&["waiting".to_string(), "ended confirmed=false".to_string(),]));
    }

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
    fn explicit_initializers_and_clones_shape_the_cellpack() {
        let options = TxOptions::default();
        assert_eq!(
            deploy_spec_with(DeployTarget::New, Some(0x7fff), &[2, 5, 1], &options),
            "[1,0,32767,2,5,1]:v0:v0"
        );
        assert_eq!(
            deploy_spec_with(DeployTarget::Reserve(65_517), Some(100), &[], &options),
            "[3,65517,100]:v0:v0"
        );
        let clone = DeployTarget::clone_of(4, 65_520).unwrap();
        assert_eq!(clone.label(), "6:65520");
        assert_eq!(
            deploy_spec_with(clone, Some(0x7fff), &[2, 9], &options),
            "[6,65520,32767,2,9]:v0:v0"
        );
        assert_eq!(DeployTarget::clone_of(2, 7).unwrap().label(), "5:7");
        assert_eq!(
            DeployTarget::clone_of(32, 0).unwrap_err().code,
            "CONFIG_INVALID"
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
