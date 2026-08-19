//! `labcoat plan` / `labcoat apply` — reconcile the HCL deployment
//! manifest (`alkanes.hcl`) against `labcoat.lock` and live chain state,
//! then execute the difference.
//!
//! Reconciliation, not replay: a deploy is complete when a chain-valid
//! lockfile record matches the local artifact; a call is complete when the
//! call journal records success for the same resolved spec on the same
//! chain instance. `labcoat reset` changes the chain identity (block 1's
//! hash), which invalidates both ledgers and makes the next apply rebuild
//! everything — that is the supported dev loop.

use crate::abi::CallArgs;
use crate::error::{LabcoatError, Result};
use crate::execute::TxOptions;
use crate::manifest::{self, Args, CallEntry, Manifest, ResolvedIds};
use crate::system::ToolkitConfig;
use crate::toolkit::{self, DeployTarget};
use crate::{abi, compile, lockfile, workspace};
use hcl::expr::Expression;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const APPLY_STATE_DIR: &str = ".labcoat/state";

// ---------------------------------------------------------------------------
// Call journal

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ApplyState {
    pub version: u32,
    /// Chain instance identity (block 1's hash) the records belong to.
    pub chain_id: Option<String>,
    pub calls: BTreeMap<String, CallRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallRecord {
    /// `started` (broadcast attempted) | `success` | `revert`
    pub status: String,
    /// Hash of the fully resolved spec + inputs + recipient, for drift
    /// detection.
    pub spec_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub txid: Option<String>,
    pub updated_at: u64,
}

fn state_path(root: &Path, network: &str) -> PathBuf {
    root.join(APPLY_STATE_DIR).join(format!("{network}.json"))
}

pub fn load_state(root: &Path, network: &str) -> Result<ApplyState> {
    let path = state_path(root, network);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ApplyState {
                version: 1,
                ..ApplyState::default()
            })
        }
        Err(e) => {
            return Err(LabcoatError::new(
                "STATE_INVALID",
                format!("cannot read {}: {e}", path.display()),
                "check permissions on .labcoat/state",
            ))
        }
    };
    serde_json::from_str(&text).map_err(|e| {
        LabcoatError::new(
            "STATE_INVALID",
            format!("{} is corrupt: {e}", path.display()),
            "repair the JSON by hand, or delete the file to forget call history (calls may re-execute)",
        )
    })
}

fn save_state(root: &Path, network: &str, state: &ApplyState) -> Result<()> {
    let path = state_path(root, network);
    let dir = path.parent().expect("state path has a parent");
    let io_err =
        |e: std::io::Error| LabcoatError::new("STATE_INVALID", e.to_string(), "check permissions");
    std::fs::create_dir_all(dir).map_err(io_err)?;
    let tmp = dir.join(format!(".{network}.json.tmp-{}", std::process::id()));
    let write = (|| {
        use std::io::Write;
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(serde_json::to_string_pretty(state).unwrap().as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&tmp, &path)
    })();
    if write.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    write.map_err(io_err)
}

// ---------------------------------------------------------------------------
// Plan model

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActionStatus {
    /// Will execute on apply.
    Pending,
    /// Already reconciled; apply skips it.
    Complete,
    /// Exists but no longer matches the manifest/artifact; apply skips it
    /// and warns rather than silently redeploying or re-calling.
    Drifted,
    /// Reserve target already deployed on-chain with matching ABI but no
    /// lockfile record; apply records it without redeploying.
    Adopt,
    /// A previous apply stopped mid-broadcast; apply refuses to continue
    /// until the transaction is verified by hand.
    Interrupted,
    /// Cannot proceed (e.g. reserve occupied by different code).
    Blocked,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedAction {
    pub kind: &'static str, // "deploy" | "call"
    pub name: String,
    pub status: ActionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Resolved or planned alkanes id (`4:N` for reserve targets).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Args after reference resolution; unresolved references stay symbolic.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spec: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wasm_sha256: Option<String>,
    #[serde(skip)]
    exec: Option<ExecDetail>,
}

#[derive(Debug, Clone)]
enum ExecDetail {
    Deploy {
        contract_index: usize,
        wasm_path: PathBuf,
        target: DeployTarget,
    },
    AdoptReserve {
        reserve: u128,
        wasm_sha256: String,
    },
    Call {
        call_index: usize,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub manifest: String,
    pub network: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_id: Option<String>,
    pub height: u64,
    pub actions: Vec<PlannedAction>,
    /// Journal records ignored because they belong to a previous chain
    /// instance (pre-reset).
    pub stale_call_records: usize,
    pub pending: usize,
}

/// Everything apply needs beyond the serializable plan.
pub struct PlanOutcome {
    pub plan: Plan,
    manifest: Manifest,
    /// Alkane ids (seeded from the manifest) plus every contract id
    /// resolvable before apply.
    resolved: ResolvedIds,
}

// ---------------------------------------------------------------------------
// Planning

struct Artifact {
    wasm_path: PathBuf,
    wasm_sha256: String,
}

/// Build every Cargo package the manifest references (one cargo invocation)
/// and hash wasm artifacts.
fn resolve_artifacts(root: &Path, manifest: &Manifest) -> Result<BTreeMap<String, Artifact>> {
    use sha2::Digest;

    let mut packages: Vec<&str> = manifest
        .contracts
        .iter()
        .filter_map(|c| c.package.as_deref())
        .collect();
    packages.sort_unstable();
    packages.dedup();

    let mut wasm_paths: BTreeMap<String, PathBuf> = BTreeMap::new();
    if !packages.is_empty() {
        let workspace = workspace::discover(root)?;
        let mut selected = Vec::new();
        for package in &packages {
            selected.extend(workspace::select(&workspace, Some(package))?);
        }
        let outcomes = compile::compile_packages(
            &workspace,
            &selected,
            &root.join("build"),
            "wasm32-unknown-unknown",
        )?;
        for outcome in outcomes {
            wasm_paths.insert(outcome.name.clone(), PathBuf::from(outcome.wasm_path));
        }
    }

    let mut artifacts = BTreeMap::new();
    for entry in &manifest.contracts {
        let wasm_path = match (&entry.package, &entry.wasm) {
            (Some(package), None) => wasm_paths.get(package).cloned().ok_or_else(|| {
                LabcoatError::new(
                    "PACKAGE_NOT_FOUND",
                    format!(
                        "contracts.{}: package `{package}` was not compiled",
                        entry.name
                    ),
                    "pass an exact Cargo contract package name",
                )
            })?,
            (None, Some(wasm)) => root.join(wasm),
            _ => unreachable!("manifest::parse enforces exactly one artifact source"),
        };
        let bytes = std::fs::read(&wasm_path).map_err(|e| {
            LabcoatError::new(
                "CONFIG_INVALID",
                format!(
                    "contracts.{}: cannot read {}: {e}",
                    entry.name,
                    wasm_path.display()
                ),
                "build the package or fix the wasm path",
            )
        })?;
        artifacts.insert(
            entry.name.clone(),
            Artifact {
                wasm_path,
                wasm_sha256: hex::encode(sha2::Sha256::digest(&bytes)),
            },
        );
    }
    Ok(artifacts)
}

/// Resolve every expression to a scalar, or None when any reference is not
/// yet resolvable.
fn resolve_args(
    args: &[Expression],
    resolved: &ResolvedIds,
    height: u64,
) -> Result<Option<Vec<String>>> {
    let mut out = Vec::with_capacity(args.len());
    for arg in args {
        match manifest::eval_scalar(arg, resolved, height)? {
            Some(value) => out.push(value),
            None => return Ok(None),
        }
    }
    Ok(Some(out))
}

/// A resolved `args` attribute, preserving its positional/named shape for
/// ABI encoding and hashing.
#[derive(Debug, Clone)]
enum ResolvedArgSet {
    Positional(Vec<String>),
    Named(Vec<(String, String)>),
}

impl ResolvedArgSet {
    fn to_call_args(&self) -> CallArgs {
        match self {
            ResolvedArgSet::Positional(values) => CallArgs::Positional(values.clone()),
            ResolvedArgSet::Named(values) => CallArgs::Named(values.clone()),
        }
    }

    /// Display form for plan output: named args render as `name=value`.
    fn display(&self) -> Vec<String> {
        match self {
            ResolvedArgSet::Positional(values) => values.clone(),
            ResolvedArgSet::Named(values) => values
                .iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect(),
        }
    }

    /// Canonical form for spec hashing: positional stays the historical
    /// comma-join (existing journals keep their hashes); named sorts by
    /// name so key order in the manifest never causes drift.
    fn canonical(&self) -> String {
        match self {
            ResolvedArgSet::Positional(values) => values.join(","),
            ResolvedArgSet::Named(values) => {
                let mut pairs: Vec<String> = values
                    .iter()
                    .map(|(name, value)| format!("{name}={value}"))
                    .collect();
                pairs.sort();
                pairs.join(",")
            }
        }
    }
}

/// Resolve an `args` attribute, or None when any reference is pending.
fn resolve_arg_set(
    args: &Args,
    resolved: &ResolvedIds,
    height: u64,
) -> Result<Option<ResolvedArgSet>> {
    match args {
        Args::Positional(items) => {
            Ok(resolve_args(items, resolved, height)?.map(ResolvedArgSet::Positional))
        }
        Args::Named(items) => {
            let mut out = Vec::with_capacity(items.len());
            for (name, expr) in items {
                match manifest::eval_scalar(expr, resolved, height)? {
                    Some(value) => out.push((name.clone(), value)),
                    None => return Ok(None),
                }
            }
            Ok(Some(ResolvedArgSet::Named(out)))
        }
    }
}

/// Args for display: resolved where possible, symbolic source text otherwise.
fn preview_args(args: &Args, resolved: &ResolvedIds, height: u64) -> Result<Vec<String>> {
    let preview =
        |expr: &Expression| -> Result<String> {
            Ok(manifest::eval_scalar(expr, resolved, height)?
                .unwrap_or_else(|| manifest::render(expr)))
        };
    match args {
        Args::Positional(items) => items.iter().map(preview).collect(),
        Args::Named(items) => items
            .iter()
            .map(|(name, expr)| Ok(format!("{name}={}", preview(expr)?)))
            .collect(),
    }
}

/// The fully resolved transaction shape of a call, or None while its
/// references are still pending.
fn resolve_call(
    call: &CallEntry,
    resolved: &ResolvedIds,
    height: u64,
) -> Result<Option<ResolvedCall>> {
    let Some(args) = resolve_arg_set(&call.args, resolved, height)? else {
        return Ok(None);
    };
    let Some(inputs) = resolve_args(&call.inputs, resolved, height)? else {
        return Ok(None);
    };
    let Some(edicts) = resolve_args(&call.edicts, resolved, height)? else {
        return Ok(None);
    };
    let Some(target) = manifest::eval_scalar(&call.contract, resolved, height)? else {
        return Ok(None);
    };
    let id = if target.contains(':') {
        target
    } else {
        match resolved.contract_id(&target) {
            Some(id) => id,
            None => return Ok(None),
        }
    };
    let to = match &call.to {
        None => None,
        Some(expr) => match manifest::eval_scalar(expr, resolved, height)? {
            Some(address) => Some(address),
            None => return Ok(None),
        },
    };
    let options = TxOptions {
        inputs: if inputs.is_empty() {
            None
        } else {
            Some(inputs.join(","))
        },
        to,
        pointer: call.pointer.clone(),
        refund: call.refund.clone(),
        edicts,
    };
    Ok(Some(ResolvedCall { id, args, options }))
}

struct ResolvedCall {
    id: String,
    args: ResolvedArgSet,
    options: TxOptions,
}

fn spec_hash(id: &str, args: &ResolvedArgSet, options: &TxOptions) -> String {
    use sha2::Digest;
    let canonical = format!(
        "{id}|{}|{}|{}|{}|{}|{}",
        args.canonical(),
        options.inputs.as_deref().unwrap_or(""),
        options.to.as_deref().unwrap_or(""),
        options.pointer.as_deref().unwrap_or(""),
        options.refund.as_deref().unwrap_or(""),
        options.edicts.join(",")
    );
    hex::encode(sha2::Sha256::digest(canonical.as_bytes()))
}

/// Chain facts gathered before reconciling.
struct ChainFacts {
    chain_id: Option<String>,
    height: u64,
    /// Reserve number -> deployed ABI bytes at `4:N`, for occupied reserves.
    reserve_abis: BTreeMap<u128, Vec<u8>>,
}

async fn gather_facts(config: &ToolkitConfig, manifest: &Manifest) -> Result<ChainFacts> {
    use alkanes_cli_common::traits::BitcoinRpcProvider;

    let provider = crate::system::connect(config, None, false).await?;
    let chain_id = BitcoinRpcProvider::get_block_hash(&provider, 1).await.ok();
    let height = BitcoinRpcProvider::get_block_count(&provider)
        .await
        .map_err(|e| LabcoatError::classify(e.into()))?;

    let mut reserve_abis = BTreeMap::new();
    for entry in &manifest.contracts {
        if let Some(reserve) = entry.reserve {
            if let Ok(bytes) = abi::fetch_deployed(config, 4, reserve).await {
                reserve_abis.insert(reserve, bytes);
            }
        }
    }
    Ok(ChainFacts {
        chain_id,
        height,
        reserve_abis,
    })
}

/// Build the plan: reconcile the manifest against the lockfile, the call
/// journal, and chain facts. Never loads a signer.
pub async fn plan(
    config: &ToolkitConfig,
    root: &Path,
    manifest_path: &Path,
) -> Result<PlanOutcome> {
    let manifest = manifest::load(manifest_path)?;
    let network = config.network_id().to_string();
    // Alkane ids resolve immediately (and unbound networks fail fast,
    // before compiling or touching the chain); contract ids join as they
    // reconcile.
    let mut resolved = ResolvedIds::from_manifest(&manifest, &network)?;
    let artifacts = resolve_artifacts(root, &manifest)?;
    let facts = gather_facts(config, &manifest).await?;
    let ledger = lockfile::load(root)?;
    let state = load_state(root, &network)?;

    let journal_stale = state.chain_id.is_some() && state.chain_id != facts.chain_id;
    let stale_call_records = if journal_stale { state.calls.len() } else { 0 };
    let calls_journal: BTreeMap<String, CallRecord> = if journal_stale {
        BTreeMap::new()
    } else {
        state.calls.clone()
    };

    let mut actions = Vec::new();

    // Contracts, in reference topology order.
    for index in manifest::deploy_order(&manifest)? {
        let entry = &manifest.contracts[index];
        let artifact = &artifacts[&entry.name];
        let target = match entry.reserve {
            Some(n) => DeployTarget::Reserve(n),
            None => DeployTarget::New,
        };
        let target_label = match target {
            DeployTarget::New => "1:0".to_string(),
            DeployTarget::Reserve(n) => format!("3:{n}"),
        };
        let recorded = ledger
            .networks
            .get(&network)
            .and_then(|n| n.get(&entry.name))
            .cloned();
        let valid_record = recorded.as_ref().filter(|dep| {
            dep.status == "success"
                && !facts
                    .chain_id
                    .as_deref()
                    .map(|chain| lockfile::is_stale(dep, chain))
                    .unwrap_or(false)
        });

        let args = preview_args(&entry.args, &resolved, facts.height)?;
        let mut action = PlannedAction {
            kind: "deploy",
            name: entry.name.clone(),
            status: ActionStatus::Pending,
            target: Some(target_label),
            id: None,
            args,
            spec: None,
            detail: None,
            wasm_sha256: Some(artifact.wasm_sha256.clone()),
            exec: None,
        };

        match valid_record {
            Some(dep) if dep.wasm_sha256.as_deref() == Some(artifact.wasm_sha256.as_str()) => {
                action.status = ActionStatus::Complete;
                action.id = Some(dep.alkanes_id.clone());
                resolved.insert_contract(&entry.name, &dep.alkanes_id)?;
            }
            Some(dep) => {
                action.status = ActionStatus::Drifted;
                action.id = Some(dep.alkanes_id.clone());
                action.detail = Some(format!(
                    "deployed wasm {} differs from local build {} — run `labcoat reset` (or use a new reserve) to redeploy",
                    dep.wasm_sha256.as_deref().unwrap_or("<unknown>"),
                    artifact.wasm_sha256
                ));
                resolved.insert_contract(&entry.name, &dep.alkanes_id)?;
            }
            None => match entry
                .reserve
                .and_then(|n| facts.reserve_abis.get(&n).map(|b| (n, b)))
            {
                Some((reserve, onchain)) => {
                    let local_abi = std::fs::read(artifact.wasm_path.with_extension("abi.json"))
                        .ok()
                        .or_else(|| abi::extract_file(&artifact.wasm_path).ok());
                    let matches = local_abi
                        .as_deref()
                        .map(|local| abi::compare(local, onchain).matches)
                        .unwrap_or(false);
                    let id = format!("4:{reserve}");
                    if matches {
                        action.status = ActionStatus::Adopt;
                        action.id = Some(id.clone());
                        action.detail = Some(
                            "already deployed on-chain with a matching ABI but missing from labcoat.lock — apply records it without redeploying"
                                .to_string(),
                        );
                        action.exec = Some(ExecDetail::AdoptReserve {
                            reserve,
                            wasm_sha256: artifact.wasm_sha256.clone(),
                        });
                        resolved.insert_contract(&entry.name, &id)?;
                    } else {
                        action.status = ActionStatus::Blocked;
                        action.detail = Some(format!(
                            "reserve {reserve} is already deployed on-chain with a different ABI — pick another reserve or `labcoat reset`"
                        ));
                    }
                }
                None => {
                    if let DeployTarget::Reserve(n) = target {
                        action.id = Some(format!("4:{n}"));
                    }
                    if let Some(resolved_args) =
                        resolve_arg_set(&entry.args, &resolved, facts.height)?
                    {
                        let constructor = abi::encode_constructor(
                            &artifact.wasm_path,
                            &resolved_args.to_call_args(),
                        )?;
                        action.spec = Some(toolkit::deploy_spec(
                            target,
                            &constructor.cellpack,
                            &TxOptions::default(),
                        ));
                    }
                    action.exec = Some(ExecDetail::Deploy {
                        contract_index: index,
                        wasm_path: artifact.wasm_path.clone(),
                        target,
                    });
                    if let DeployTarget::Reserve(n) = target {
                        // Downstream entries can rely on the planned id even
                        // before the deploy runs.
                        resolved.insert_contract(&entry.name, &format!("4:{n}"))?;
                    }
                }
            },
        }
        actions.push(action);
    }

    // Calls, in declaration order.
    for (call_index, call) in manifest.calls.iter().enumerate() {
        let mut action = PlannedAction {
            kind: "call",
            name: call.label.clone(),
            status: ActionStatus::Pending,
            target: None,
            id: None,
            args: preview_args(&call.args, &resolved, facts.height)?,
            spec: None,
            detail: None,
            wasm_sha256: None,
            exec: Some(ExecDetail::Call { call_index }),
        };
        let resolved_call = resolve_call(call, &resolved, facts.height)?;
        if let Some(rc) = &resolved_call {
            action.id = Some(rc.id.clone());
            action.args = rc.args.display();
        }
        match calls_journal.get(&call.label) {
            Some(record) if record.status == "started" => {
                action.status = ActionStatus::Interrupted;
                action.detail = Some(format!(
                    "a previous apply stopped mid-broadcast{} — verify on-chain, then delete the `{}` entry from .labcoat/state/{network}.json to retry",
                    record
                        .txid
                        .as_deref()
                        .map(|t| format!(" (txid {t})"))
                        .unwrap_or_default(),
                    call.label,
                ));
            }
            Some(record) if record.status == "success" => match &resolved_call {
                Some(rc) if spec_hash(&rc.id, &rc.args, &rc.options) == record.spec_hash => {
                    action.status = ActionStatus::Complete;
                }
                Some(_) => {
                    action.status = ActionStatus::Drifted;
                    action.detail = Some(
                        "already executed with different arguments — edit the label to run it as a new call, or delete its journal entry to re-execute"
                            .to_string(),
                    );
                }
                None => {
                    // References pending: chain identity matched, so the
                    // journal is current; treat as complete.
                    action.status = ActionStatus::Complete;
                }
            },
            _ => {}
        }
        actions.push(action);
    }

    let pending = actions
        .iter()
        .filter(|a| matches!(a.status, ActionStatus::Pending | ActionStatus::Adopt))
        .count();
    Ok(PlanOutcome {
        plan: Plan {
            manifest: manifest_path.display().to_string(),
            network,
            chain_id: facts.chain_id,
            height: facts.height,
            actions,
            stale_call_records,
            pending,
        },
        manifest,
        resolved,
    })
}

// ---------------------------------------------------------------------------
// Apply

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedAction {
    pub kind: &'static str,
    pub name: String,
    /// applied | adopted | skipped-complete | skipped-drifted
    pub outcome: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub txid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fee: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyReport {
    pub network: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_id: Option<String>,
    pub actions: Vec<AppliedAction>,
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn stop(kind: &str, name: &str, error: LabcoatError) -> LabcoatError {
    LabcoatError::new(
        error.code,
        format!("apply stopped at {kind} `{name}`: {}", error.message),
        "completed actions are recorded; fix the cause and re-run apply to resume",
    )
}

/// Execute the plan's pending actions in order. Requires broadcast intent —
/// the CLI only calls this under `--broadcast`.
pub async fn apply(
    config: &ToolkitConfig,
    signer: &crate::signer::SignerSpec,
    root: &Path,
    manifest_path: &Path,
) -> Result<ApplyReport> {
    let PlanOutcome {
        plan,
        manifest,
        mut resolved,
    } = plan(config, root, manifest_path).await?;

    // Fail fast on states that need a human before any broadcast.
    for action in &plan.actions {
        match action.status {
            ActionStatus::Interrupted | ActionStatus::Blocked => {
                return Err(LabcoatError::new(
                    "APPLY_BLOCKED",
                    format!(
                        "{} `{}` cannot proceed: {}",
                        action.kind,
                        action.name,
                        action.detail.as_deref().unwrap_or("unknown reason")
                    ),
                    "resolve the reported state and re-run apply",
                ));
            }
            _ => {}
        }
    }

    let network = plan.network.clone();
    let mut state = load_state(root, &network)?;
    if state.chain_id != plan.chain_id {
        state = ApplyState {
            version: 1,
            chain_id: plan.chain_id.clone(),
            calls: BTreeMap::new(),
        };
    }

    let mut report = ApplyReport {
        network: network.clone(),
        chain_id: plan.chain_id.clone(),
        actions: Vec::new(),
    };

    for action in &plan.actions {
        match action.status {
            ActionStatus::Complete => {
                report.actions.push(AppliedAction {
                    kind: action.kind,
                    name: action.name.clone(),
                    outcome: "skipped-complete".into(),
                    id: action.id.clone(),
                    txid: None,
                    fee: None,
                    detail: None,
                });
                continue;
            }
            ActionStatus::Drifted => {
                report.actions.push(AppliedAction {
                    kind: action.kind,
                    name: action.name.clone(),
                    outcome: "skipped-drifted".into(),
                    id: action.id.clone(),
                    txid: None,
                    fee: None,
                    detail: action.detail.clone(),
                });
                continue;
            }
            _ => {}
        }
        match action
            .exec
            .as_ref()
            .expect("pending actions carry exec detail")
        {
            ExecDetail::AdoptReserve {
                reserve,
                wasm_sha256,
            } => {
                let id = format!("4:{reserve}");
                // Durable-state guard before the adoption is recorded: a
                // reset or foreign chain aborts this action, and the lease
                // covers both ledger writes.
                let mut state_guard = crate::state::deploy_guard(
                    root,
                    &config.environment,
                    &crate::state::observed_chain(config, plan.chain_id.clone()),
                )
                .map_err(|e| stop("deploy", &action.name, e))?;
                let deployment = lockfile::Deployment {
                    alkanes_id: id.clone(),
                    wasm_sha256: Some(wasm_sha256.clone()),
                    txid: "adopted".into(),
                    block: None,
                    status: "success".into(),
                    deployed_at: now_millis(),
                    chain_id: plan.chain_id.clone(),
                };
                lockfile::record(root, &network, &action.name, deployment.clone())
                    .map_err(|e| stop("deploy", &action.name, e))?;
                if let Some((mut lease, v2_state)) = state_guard.take() {
                    if let Err(e) = crate::state::record_instance(
                        &mut lease,
                        v2_state,
                        &action.name,
                        &deployment,
                        crate::state::InstanceOrigin::Adopted,
                        crate::state::InstanceExtras {
                            commit_txid: None,
                            labcoat_version: Some(env!("CARGO_PKG_VERSION").to_string()),
                            revert_reason: None,
                        },
                    ) {
                        tracing::warn!("durable state not updated: {e}");
                    }
                }
                resolved
                    .insert_contract(&action.name, &id)
                    .map_err(|e| stop("deploy", &action.name, e))?;
                report.actions.push(AppliedAction {
                    kind: "deploy",
                    name: action.name.clone(),
                    outcome: "adopted".into(),
                    id: Some(id),
                    txid: None,
                    fee: None,
                    detail: action.detail.clone(),
                });
            }
            ExecDetail::Deploy {
                contract_index,
                wasm_path,
                target,
            } => {
                let entry = &manifest.contracts[*contract_index];
                let args =
                    resolve_arg_set(&entry.args, &resolved, plan.height)?.ok_or_else(|| {
                        stop(
                            "deploy",
                            &entry.name,
                            LabcoatError::new(
                                "APPLY_BLOCKED",
                                "constructor references a contract that has no resolved id",
                                "deploy the referenced contract first",
                            ),
                        )
                    })?;
                let constructor = abi::encode_constructor(wasm_path, &args.to_call_args())
                    .map_err(|e| stop("deploy", &entry.name, e))?;
                let outcome = toolkit::deploy_in(
                    config,
                    signer,
                    root,
                    toolkit::DeployRequest {
                        wasm_path,
                        contract_name: Some(entry.name.clone()),
                        cellpack_args: &constructor.cellpack,
                        fee_rate: config.fee_rate,
                        target: *target,
                        options: &TxOptions::default(),
                    },
                )
                .await
                .map_err(|e| stop("deploy", &entry.name, e))?;
                if outcome.status != "success" {
                    return Err(stop(
                        "deploy",
                        &entry.name,
                        LabcoatError::new(
                            "EXECUTION_REVERT",
                            format!(
                                "deployment reverted: {}",
                                outcome.revert_reason.as_deref().unwrap_or("unknown reason")
                            ),
                            "inspect the trace with `labcoat trace <txid>`",
                        ),
                    ));
                }
                if let Some(id) = &outcome.alkanes_id {
                    resolved
                        .insert_contract(&entry.name, id)
                        .map_err(|e| stop("deploy", &entry.name, e))?;
                }
                report.actions.push(AppliedAction {
                    kind: "deploy",
                    name: entry.name.clone(),
                    outcome: "applied".into(),
                    id: outcome.alkanes_id.clone(),
                    txid: Some(outcome.txid.clone()),
                    fee: Some(outcome.fee),
                    detail: None,
                });
            }
            ExecDetail::Call { call_index } => {
                let call = &manifest.calls[*call_index];
                let rc = resolve_call(call, &resolved, plan.height)?.ok_or_else(|| {
                    stop(
                        "call",
                        &call.label,
                        LabcoatError::new(
                            "APPLY_BLOCKED",
                            "call references a contract that has no resolved id",
                            "deploy the referenced contract first",
                        ),
                    )
                })?;
                let (block, tx) = toolkit::parse_alkanes_id(&rc.id)?;
                let hash = spec_hash(&rc.id, &rc.args, &rc.options);

                // Journal the attempt before broadcasting, so a crash
                // between broadcast and confirmation is visible as
                // `started` instead of silently re-executing next run.
                state.calls.insert(
                    call.label.clone(),
                    CallRecord {
                        status: "started".into(),
                        spec_hash: hash.clone(),
                        txid: None,
                        updated_at: now_millis(),
                    },
                );
                save_state(root, &network, &state)?;

                // Resolve method: ABI name against the deployed contract, or
                // a numeric opcode.
                let (opcode, cellpack_args) = if let Ok(op) = call.method.parse::<u128>() {
                    let ResolvedArgSet::Positional(values) = &rc.args else {
                        return Err(stop(
                            "call",
                            &call.label,
                            LabcoatError::new(
                                "CONFIG_INVALID",
                                "named args require an ABI method name — numeric opcode calls take positional raw cellpack args",
                                "use the ABI method name, or positional args = [...]",
                            ),
                        ));
                    };
                    let raw = values
                        .iter()
                        .map(|a| abi::parse_raw_arg(a))
                        .collect::<Result<Vec<_>>>()
                        .map_err(|e| stop("call", &call.label, e))?;
                    (op, raw)
                } else {
                    let abi_bytes = abi::fetch_deployed(config, block, tx)
                        .await
                        .map_err(|e| stop("call", &call.label, e))?;
                    let method =
                        abi::resolve_method(&abi_bytes, &call.method, &rc.args.to_call_args())
                            .map_err(|e| stop("call", &call.label, e))?;
                    (method.opcode, method.cellpack_args)
                };

                let result = toolkit::call(
                    config,
                    signer,
                    toolkit::CallRequest {
                        block,
                        tx,
                        opcode,
                        args: &cellpack_args,
                        fee_rate: config.fee_rate,
                        options: &rc.options,
                    },
                )
                .await;

                let outcome = match result {
                    Ok(outcome) => outcome,
                    Err(e) => {
                        // Leave the `started` record in place: the broadcast
                        // may or may not have landed.
                        return Err(stop("call", &call.label, e));
                    }
                };
                let succeeded = outcome.status == "success";
                state.calls.insert(
                    call.label.clone(),
                    CallRecord {
                        status: if succeeded { "success" } else { "revert" }.into(),
                        spec_hash: hash,
                        txid: Some(outcome.txid.clone()),
                        updated_at: now_millis(),
                    },
                );
                save_state(root, &network, &state)?;
                if !succeeded {
                    return Err(stop(
                        "call",
                        &call.label,
                        LabcoatError::new(
                            "EXECUTION_REVERT",
                            format!(
                                "call reverted: {}",
                                outcome.revert_reason.as_deref().unwrap_or("unknown reason")
                            ),
                            "inspect the trace with `labcoat trace <txid>`",
                        ),
                    ));
                }
                report.actions.push(AppliedAction {
                    kind: "call",
                    name: call.label.clone(),
                    outcome: "applied".into(),
                    id: Some(rc.id.clone()),
                    txid: Some(outcome.txid.clone()),
                    fee: Some(outcome.fee),
                    detail: None,
                });
            }
        }
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn positional(values: &[&str]) -> ResolvedArgSet {
        ResolvedArgSet::Positional(values.iter().map(|v| v.to_string()).collect())
    }

    fn named(values: &[(&str, &str)]) -> ResolvedArgSet {
        ResolvedArgSet::Named(
            values
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        )
    }

    #[test]
    fn spec_hash_tracks_every_transaction_shaping_field() {
        let base = TxOptions::default();
        let a = spec_hash("4:1", &positional(&["1"]), &base);
        assert_eq!(a, spec_hash("4:1", &positional(&["1"]), &base));
        assert_ne!(a, spec_hash("4:2", &positional(&["1"]), &base));
        assert_ne!(a, spec_hash("4:1", &positional(&["2"]), &base));
        let with_to = TxOptions {
            to: Some("bcrt1q".into()),
            ..TxOptions::default()
        };
        assert_ne!(a, spec_hash("4:1", &positional(&["1"]), &with_to));
    }

    #[test]
    fn named_spec_hashes_are_key_order_independent() {
        let base = TxOptions::default();
        let a = spec_hash("4:1", &named(&[("strike", "75"), ("supply", "100")]), &base);
        let b = spec_hash("4:1", &named(&[("supply", "100"), ("strike", "75")]), &base);
        assert_eq!(a, b);
        assert_ne!(
            a,
            spec_hash("4:1", &named(&[("strike", "76"), ("supply", "100")]), &base)
        );
        // Named and positional shapes hash differently by design.
        assert_ne!(a, spec_hash("4:1", &positional(&["75", "100"]), &base));
    }

    #[test]
    fn state_round_trips_atomically_and_rejects_corruption() {
        let root = std::env::temp_dir().join(format!("labcoat-apply-state-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();

        assert!(load_state(&root, "labcoat").unwrap().calls.is_empty());
        let mut state = ApplyState {
            version: 1,
            chain_id: Some("hash-a".into()),
            calls: BTreeMap::new(),
        };
        state.calls.insert(
            "fund".into(),
            CallRecord {
                status: "success".into(),
                spec_hash: "abc".into(),
                txid: Some("00".into()),
                updated_at: 1,
            },
        );
        save_state(&root, "labcoat", &state).unwrap();
        let loaded = load_state(&root, "labcoat").unwrap();
        assert_eq!(loaded.chain_id.as_deref(), Some("hash-a"));
        assert_eq!(loaded.calls["fund"].status, "success");

        let entries: Vec<_> = std::fs::read_dir(root.join(APPLY_STATE_DIR))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(entries, vec!["labcoat.json".to_string()]);

        std::fs::write(state_path(&root, "labcoat"), "{ nope").unwrap();
        assert_eq!(
            load_state(&root, "labcoat").unwrap_err().code,
            "STATE_INVALID"
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn call_resolution_waits_for_pending_references() {
        let manifest = manifest::parse(
            r#"
contract "fire" {
  package = "fire"
}

contract "series" {
  package = "series"
}

call "fund" {
  contract = "series"
  method   = "fund"
  inputs   = ["${contract.fire.id}:100"]
}
"#,
        )
        .unwrap();
        let call = &manifest.calls[0];
        let mut resolved = ResolvedIds::from_manifest(&manifest, "labcoat").unwrap();
        resolved.insert_contract("series", "4:65014").unwrap();
        // fire still pending → not resolvable yet
        assert!(resolve_call(call, &resolved, 10).unwrap().is_none());

        resolved.insert_contract("fire", "4:65011").unwrap();
        let rc = resolve_call(call, &resolved, 10).unwrap().unwrap();
        assert_eq!(rc.id, "4:65014");
        assert_eq!(rc.options.inputs.as_deref(), Some("4:65011:100"));
    }

    #[test]
    fn height_references_resolve_at_plan_height() {
        let manifest = manifest::parse(
            r#"
alkane "usd" {
  id = [4, 65012]
}

contract "token" {
  package = "token"
}

contract "series" {
  package = "series"
  args    = [contract.token.id, alkane.usd.id, height + 100, 75]
}
"#,
        )
        .unwrap();
        let mut resolved = ResolvedIds::from_manifest(&manifest, "labcoat").unwrap();
        resolved.insert_contract("token", "4:65011").unwrap();
        let out = resolve_arg_set(&manifest.contracts[1].args, &resolved, 423)
            .unwrap()
            .unwrap();
        assert_eq!(out.display(), ["4:65011", "4:65012", "523", "75"]);

        // Unresolved contract reference → the whole set is not resolvable;
        // alkane references alone never block.
        let fresh = ResolvedIds::from_manifest(&manifest, "labcoat").unwrap();
        assert!(resolve_arg_set(&manifest.contracts[1].args, &fresh, 423)
            .unwrap()
            .is_none());
    }

    #[test]
    fn named_args_resolve_and_preview_as_pairs() {
        let manifest = manifest::parse(
            r#"
alkane "usd" {
  id = [4, 65012]
}

contract "series" {
  package = "series"
  args = {
    quote  = alkane.usd.id
    expiry = height + 100
  }
}
"#,
        )
        .unwrap();
        let resolved = ResolvedIds::from_manifest(&manifest, "labcoat").unwrap();
        let set = resolve_arg_set(&manifest.contracts[0].args, &resolved, 400)
            .unwrap()
            .unwrap();
        assert_eq!(set.display(), ["quote=4:65012", "expiry=500"]);
        assert_eq!(
            preview_args(&manifest.contracts[0].args, &resolved, 400).unwrap(),
            ["quote=4:65012", "expiry=500"]
        );
    }
}
