//! `labcoat plan` / `labcoat apply` — reconcile the deployment graph a
//! project's `deploy.rs` declares against the network's run file and
//! chain state, then execute what has not run.
//!
//! A ledger, not convergence: Alkanes deployments are one-shot and
//! immutable, so a pinned step runs once and is then compared with its
//! record. An edited step is a `conflict` apply refuses, never a diff it
//! repairs; `--replace <name>` deploys a new instance and keeps the old
//! record. Only two things change without that flag: a replaceable
//! implementation (new code deploys a new instance) and a setting (a
//! mutable value written when a read shows it differs).

use crate::abi::CallArgs;
use crate::deployment::{
    self, definition_hash, reference_identity, reference_key, Args, Contract, Graph, Lifecycle,
    Resolved, Setting, Source, Step, TxShape, ValueType,
};
use crate::error::{LabcoatError, Result};
use crate::execute::TxOptions;
use crate::runfile::{self, RecordStatus, RunFile, StepRecord};
use crate::system::ToolkitConfig;
use crate::toolkit::{self, DeployTarget};
use crate::{abi, compile, lockfile, workspace};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Choices a human makes per invocation.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Pinned steps to run again: a contract deploys a new instance, a
    /// call executes again.
    pub replace: BTreeSet<String>,
}

// ---------------------------------------------------------------------------
// Plan model

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActionStatus {
    /// Runs on the next `apply --broadcast`.
    Pending,
    /// Already ran with this definition; apply skips it.
    Complete,
    /// Ran with a different definition, or references an instance that
    /// was since replaced. Apply refuses until the step is restored or
    /// named in `--replace`.
    Conflict,
    /// A reserved id already holds matching code with no record; apply
    /// records it without redeploying.
    Adopt,
    /// A previous apply stopped mid-broadcast; verify that transaction
    /// before continuing.
    Interrupted,
    /// Cannot proceed (a reserve holds different code, a read failed).
    Blocked,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedAction {
    /// `external` | `contract` | `call` | `setting`
    pub kind: &'static str,
    pub name: String,
    pub status: ActionStatus,
    /// What a pending step will do: `create` | `clone` | `replace` |
    /// `adopt` | `call` | `rerun` | `update` | `check` (a setting whose
    /// contract does not exist yet); `none` otherwise.
    pub action: &'static str,
    /// The deploy cellpack target (`1:0`, `3:N`, `5:N`, `6:N`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// The step's Alkane id, when known before apply.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Arguments with resolvable references resolved; the rest stay
    /// symbolic (`<token.id>`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wasm_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub script: String,
    pub network: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_id: Option<String>,
    pub height: u64,
    pub actions: Vec<PlannedAction>,
    /// Run-file records ignored because they belong to a previous chain
    /// instance (before a reset).
    pub stale_records: usize,
    /// Recorded steps the script no longer declares.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub undeclared: Vec<String>,
    pub pending: usize,
    pub conflicts: usize,
}

/// Everything apply needs beyond the serializable plan.
pub struct PlanOutcome {
    pub plan: Plan,
    artifacts: BTreeMap<String, Artifact>,
    resolved: Resolved,
    run: RunFile,
}

#[derive(Debug, Clone)]
struct Artifact {
    wasm_path: PathBuf,
    wasm_sha256: String,
    abi: Option<Vec<u8>>,
}

impl Artifact {
    fn stem(&self) -> Option<String> {
        self.wasm_path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
    }
}

/// A setting's current on-chain value, read during planning.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Reading {
    Value(String),
    Failed(String),
}

/// Chain facts gathered before reconciling.
#[derive(Debug, Clone, Default)]
struct Facts {
    chain_id: Option<String>,
    height: u64,
    /// Reserve number → deployed ABI bytes at `4:N`, for occupied reserves.
    reserve_abis: BTreeMap<u128, Vec<u8>>,
    /// Setting name → current value, for settings with a read whose target
    /// is known before apply.
    readings: BTreeMap<String, Reading>,
}

// ---------------------------------------------------------------------------
// Artifacts

/// Build every Cargo package the graph references (one cargo invocation)
/// and hash the wasm artifacts.
fn resolve_artifacts(root: &Path, graph: &Graph) -> Result<BTreeMap<String, Artifact>> {
    use sha2::Digest;

    let contracts: Vec<&Contract> = graph
        .steps
        .iter()
        .filter_map(|step| match step {
            Step::Contract(contract) => Some(contract),
            _ => None,
        })
        .collect();
    let mut packages: Vec<&str> = contracts
        .iter()
        .filter_map(|contract| match &contract.source {
            Source::Package { package } => Some(package.as_str()),
            _ => None,
        })
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
    for contract in contracts {
        let wasm_path = match &contract.source {
            Source::Package { package } => wasm_paths.get(package).cloned().ok_or_else(|| {
                LabcoatError::new(
                    "PACKAGE_NOT_FOUND",
                    format!(
                        "contract `{}`: package `{package}` was not compiled",
                        contract.name
                    ),
                    "pass an exact Cargo contract package name",
                )
            })?,
            Source::Wasm { wasm } => root.join(wasm),
            Source::Clone { .. } => continue,
        };
        let bytes = std::fs::read(&wasm_path).map_err(|e| {
            LabcoatError::new(
                "CONFIG_INVALID",
                format!(
                    "contract `{}`: cannot read {}: {e}",
                    contract.name,
                    wasm_path.display()
                ),
                "build the package or fix the wasm path",
            )
        })?;
        artifacts.insert(
            contract.name.clone(),
            Artifact {
                abi: abi::artifact_abi(&wasm_path),
                wasm_sha256: hex::encode(sha2::Sha256::digest(&bytes)),
                wasm_path,
            },
        );
    }
    Ok(artifacts)
}

// ---------------------------------------------------------------------------
// Argument resolution

fn preview_args(args: &Args, resolved: &Resolved, height: u64) -> Vec<String> {
    let mut out = Vec::new();
    for (name, value) in args.named.iter().flatten() {
        out.push(format!("{name}={}", resolved.preview(value, height)));
    }
    for value in args.positional.iter().flatten() {
        out.push(resolved.preview(value, height));
    }
    out
}

fn unresolved(kind: &str, name: &str) -> LabcoatError {
    LabcoatError::new(
        "APPLY_BLOCKED",
        format!("{kind} `{name}` references a step that has no id yet"),
        "run the referenced step first — `labcoat plan` shows what is pending",
    )
}

/// Evaluate args to the form the ABI encoder takes, or None while any
/// reference is unresolved.
fn resolve_args(args: &Args, resolved: &Resolved, height: u64) -> Option<CallArgs> {
    if let Some(named) = &args.named {
        return named
            .iter()
            .map(|(name, value)| Some((name.clone(), resolved.eval(value, height)?)))
            .collect::<Option<Vec<_>>>()
            .map(CallArgs::Named);
    }
    args.positional
        .iter()
        .flatten()
        .map(|value| resolved.eval(value, height))
        .collect::<Option<Vec<_>>>()
        .map(CallArgs::Positional)
}

fn display_args(args: &CallArgs) -> Vec<String> {
    match args {
        CallArgs::Positional(values) => values.clone(),
        CallArgs::Named(values) => values
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect(),
    }
}

fn resolve_tx(tx: &TxShape, resolved: &Resolved, height: u64) -> Option<TxOptions> {
    let inputs = tx
        .inputs
        .iter()
        .map(|value| resolved.eval(value, height))
        .collect::<Option<Vec<_>>>()?;
    let edicts = tx
        .edicts
        .iter()
        .map(|value| resolved.eval(value, height))
        .collect::<Option<Vec<_>>>()?;
    let to = match &tx.to {
        None => None,
        Some(value) => Some(resolved.eval(value, height)?),
    };
    Some(TxOptions {
        inputs: (!inputs.is_empty()).then(|| inputs.join(",")),
        to,
        pointer: tx.pointer.clone(),
        refund: tx.refund.clone(),
        edicts,
    })
}

/// The id of every step a step references, as resolved right now.
fn current_refs(step: &Step, resolved: &Resolved) -> BTreeMap<String, String> {
    step.references()
        .into_iter()
        .filter_map(|reference| {
            Some((
                reference_key(reference),
                reference_identity(resolved, reference)?,
            ))
        })
        .collect()
}

fn deploy_target(
    contract: &Contract,
    resolved: &Resolved,
    height: u64,
) -> Result<Option<DeployTarget>> {
    match &contract.source {
        Source::Clone { of } => match resolved.eval(of, height) {
            Some(id) => {
                let (block, tx) = deployment::parse_id(&id)?;
                Ok(Some(DeployTarget::clone_of(block, tx)?))
            }
            None => Ok(None),
        },
        _ => Ok(Some(match contract.reserve_number()? {
            Some(number) => DeployTarget::Reserve(number),
            None => DeployTarget::New,
        })),
    }
}

// ---------------------------------------------------------------------------
// Reconciliation (pure)

fn lifecycles(graph: &Graph) -> BTreeMap<&str, Lifecycle> {
    graph
        .steps
        .iter()
        .filter_map(|step| match step {
            Step::Contract(contract) => Some((contract.name.as_str(), contract.lifecycle)),
            _ => None,
        })
        .collect()
}

/// Why a recorded step no longer matches the instances it references:
/// one line per reference that was pinned to an id that has since
/// changed. A replaceable implementation never counts — reaching it
/// through a pointer is the point of declaring it replaceable.
fn stale_refs(
    step: &Step,
    record: &StepRecord,
    resolved: &Resolved,
    lifecycles: &BTreeMap<&str, Lifecycle>,
) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for reference in step.references() {
        let key = reference_key(reference);
        if !seen.insert(key.clone()) {
            continue;
        }
        if lifecycles.get(reference.step.as_str()) == Some(&Lifecycle::Replaceable) {
            continue;
        }
        let Some(recorded) = record.refs.get(&key) else {
            continue;
        };
        match reference_identity(resolved, reference) {
            Some(current) if &current == recorded => {}
            Some(current) => out.push(format!(
                "references `{}`, recorded as {recorded} and now {current}",
                reference.step
            )),
            None => out.push(format!(
                "references `{}` ({recorded}), which is being replaced",
                reference.step
            )),
        }
    }
    out
}

fn conflict_detail(name: &str, changed: bool, stale: &[String], rerun: bool) -> String {
    let mut reasons = Vec::new();
    if changed {
        reasons.push("its definition changed since it ran".to_string());
    }
    reasons.extend(stale.iter().cloned());
    let fix = if rerun {
        format!("restore it, or pass `--replace {name}` to run it again")
    } else {
        format!("restore it, or pass `--replace {name}` to deploy a new instance")
    };
    format!("{} — {fix}", reasons.join("; "))
}

/// Ids known before any step runs: externals for this network and every
/// successfully recorded contract. Used to gather setting reads.
fn known_ids(graph: &Graph, network: &str, run: &RunFile) -> Resolved {
    let mut resolved = Resolved::default();
    for step in &graph.steps {
        match step {
            Step::External(external) => {
                if let Some((block, tx)) = external.id_for(network) {
                    let _ = resolved.insert(&external.name, &format!("{block}:{tx}"), &[]);
                }
            }
            Step::Contract(contract) => {
                if let Some(record) = success_record(run, &contract.name, "contract") {
                    if let Some(id) = &record.id {
                        let _ = resolved.insert(&contract.name, id, &record.created);
                    }
                }
            }
            _ => {}
        }
    }
    resolved
}

fn record_of<'a>(run: &'a RunFile, name: &str, kind: &str) -> Option<&'a StepRecord> {
    run.steps.get(name).filter(|record| record.kind == kind)
}

fn success_record<'a>(run: &'a RunFile, name: &str, kind: &str) -> Option<&'a StepRecord> {
    record_of(run, name, kind).filter(|record| record.status == RecordStatus::Success)
}

/// A read-less setting's current value: what Labcoat last recorded for
/// this target instance, or — the first time — what the target's
/// initializer was given, evaluated from the ids the target consumed when
/// it was created.
fn recorded_setting_value(
    setting: &Setting,
    target_id: &str,
    run: &RunFile,
    height: u64,
) -> Option<String> {
    if let Some(record) = success_record(run, &setting.name, "setting") {
        if record.id.as_deref() == Some(target_id) {
            return record.value.clone();
        }
    }
    let mut references = Vec::new();
    setting.contract.references(&mut references);
    let target_step = &references.first()?.step;
    let target_record = success_record(run, target_step, "contract")?;
    if target_record.id.as_deref() != Some(target_id) {
        return None;
    }
    let mut at_creation = Resolved::default();
    for (key, id) in &target_record.refs {
        if !key.contains('#') {
            at_creation.insert(key, id, &[]).ok()?;
        }
    }
    at_creation.eval(&setting.value, height)
}

struct Reconciled {
    actions: Vec<PlannedAction>,
    resolved: Resolved,
}

fn reconcile(
    graph: &Graph,
    network: &str,
    artifacts: &BTreeMap<String, Artifact>,
    facts: &Facts,
    run: &RunFile,
    options: &Options,
) -> Result<Reconciled> {
    for name in &options.replace {
        match graph.steps.iter().find(|step| step.name() == name) {
            Some(Step::Contract(_) | Step::Call(_)) => {}
            Some(other) => {
                return Err(LabcoatError::new(
                    "CONFIG_INVALID",
                    format!(
                    "--replace {name}: `{name}` is {} — only contracts and calls can be replaced",
                    match other.kind() {
                        "external" => "an external reference",
                        _ => "a setting",
                    }
                ),
                    "name a contract or call step from deploy.rs",
                ))
            }
            None => {
                return Err(LabcoatError::new(
                    "CONFIG_INVALID",
                    format!("--replace {name}: deploy.rs declares no step named `{name}`"),
                    "run `labcoat plan` to list the declared steps",
                ))
            }
        }
    }

    let lifecycles = lifecycles(graph);
    let height = facts.height;
    let mut resolved = Resolved::default();
    let mut actions = Vec::new();

    for step in &graph.steps {
        let name = step.name();
        let replace = options.replace.contains(name);
        let mut action = PlannedAction {
            kind: step.kind(),
            name: name.to_string(),
            status: ActionStatus::Pending,
            action: "none",
            target: None,
            id: None,
            args: Vec::new(),
            detail: None,
            wasm_sha256: None,
        };

        match step {
            Step::External(external) => {
                let (block, tx) = external.id_for(network).ok_or_else(|| {
                    LabcoatError::new(
                        "CONFIG_INVALID",
                        format!(
                            "external `{name}` has no id bound for network `{network}` (bound: {})",
                            external.ids.keys().cloned().collect::<Vec<_>>().join(", ")
                        ),
                        "bind the id for this network in deploy.rs with `.on(network, block, tx)`",
                    )
                })?;
                let id = format!("{block}:{tx}");
                resolved.insert(name, &id, &[])?;
                action.status = ActionStatus::Complete;
                action.id = Some(id);
            }

            Step::Contract(contract) => {
                let artifact = artifacts.get(name);
                let wasm_sha256 = artifact.map(|artifact| artifact.wasm_sha256.as_str());
                let definition = definition_hash(step, wasm_sha256);
                let target = deploy_target(contract, &resolved, height)?;
                let is_clone = matches!(contract.source, Source::Clone { .. });
                action.target = target.map(DeployTarget::label);
                action.args = preview_args(&contract.init.args, &resolved, height);
                action.wasm_sha256 = wasm_sha256.map(String::from);
                let create = if is_clone { "clone" } else { "create" };
                let reserve = match target {
                    Some(DeployTarget::Reserve(number)) => Some(number),
                    _ => None,
                };

                let record = record_of(run, name, "contract");
                let pending_new =
                    |action: &mut PlannedAction, resolved: &mut Resolved| -> Result<()> {
                        match reserve {
                            Some(number) => {
                                // Downstream steps can rely on a reserved id
                                // before the deploy runs.
                                let id = format!("4:{number}");
                                resolved.insert(name, &id, std::slice::from_ref(&id))?;
                                action.id = Some(id);
                            }
                            None => resolved.remove(name),
                        }
                        Ok(())
                    };

                match record {
                    Some(record) if record.status == RecordStatus::Started && !replace => {
                        action.status = ActionStatus::Interrupted;
                        action.detail = Some(format!(
                            "a previous apply stopped mid-broadcast — check the wallet's recent transactions, then `labcoat import {name} <block:tx>` if the deploy landed, or pass `--replace {name}` to broadcast it again"
                        ));
                        resolved.remove(name);
                    }
                    Some(record) if record.status == RecordStatus::Success => {
                        let changed = record.definition != definition;
                        let stale = stale_refs(step, record, &resolved, &lifecycles);
                        let recorded_id = record.id.clone().unwrap_or_default();
                        let reserve_taken =
                            reserve.is_some_and(|number| recorded_id == format!("4:{number}"));
                        let replaceable = contract.lifecycle == Lifecycle::Replaceable;
                        if !replace && !changed && stale.is_empty() {
                            action.status = ActionStatus::Complete;
                            if !record.args.is_empty() {
                                // Show what ran, not today's evaluation.
                                action.args = record.args.clone();
                            }
                            action.id = Some(recorded_id.clone());
                            resolved.insert(name, &recorded_id, &record.created)?;
                        } else if (replace || replaceable) && reserve_taken {
                            action.status = ActionStatus::Blocked;
                            action.id = Some(recorded_id.clone());
                            action.detail = Some(format!(
                                "reserve {} already holds the instance being replaced — give the new instance another reserve number",
                                reserve.unwrap_or_default()
                            ));
                            resolved.insert(name, &recorded_id, &record.created)?;
                        } else if replace || replaceable {
                            action.action = "replace";
                            action.detail = Some(if replace {
                                format!("replaces {recorded_id}, which stays on chain and in the record's history")
                            } else if changed {
                                format!("the code changed — deploys a new instance and keeps {recorded_id} in the record's history")
                            } else {
                                format!("{} — deploys a new instance", stale.join("; "))
                            });
                            pending_new(&mut action, &mut resolved)?;
                        } else {
                            action.status = ActionStatus::Conflict;
                            action.id = Some(recorded_id.clone());
                            action.detail = Some(conflict_detail(name, changed, &stale, false));
                            resolved.insert(name, &recorded_id, &record.created)?;
                        }
                    }
                    _ => {
                        let occupied = reserve.and_then(|number| {
                            facts.reserve_abis.get(&number).map(|abi| (number, abi))
                        });
                        match occupied {
                            Some((number, onchain)) => {
                                let id = format!("4:{number}");
                                let matches = artifact
                                    .and_then(|artifact| artifact.abi.as_deref())
                                    .map(|local| abi::compare(local, onchain).matches)
                                    .unwrap_or(false);
                                action.id = Some(id.clone());
                                if matches {
                                    action.status = ActionStatus::Adopt;
                                    action.action = "adopt";
                                    action.detail = Some(
                                        "already deployed on-chain with a matching ABI but not recorded — apply records it without redeploying"
                                            .to_string(),
                                    );
                                    resolved.insert(name, &id, std::slice::from_ref(&id))?;
                                } else {
                                    action.status = ActionStatus::Blocked;
                                    action.detail = Some(format!(
                                        "reserve {number} is already deployed on-chain with a different ABI — pick another reserve or `labcoat reset`"
                                    ));
                                    resolved.remove(name);
                                }
                            }
                            None => {
                                action.action = create;
                                pending_new(&mut action, &mut resolved)?;
                            }
                        }
                    }
                }
            }

            Step::Call(call) => {
                let definition = definition_hash(step, None);
                action.args = preview_args(&call.call.args, &resolved, height);
                action.id = resolved.eval(&call.contract, height);
                match record_of(run, name, "call") {
                    Some(record) if record.status == RecordStatus::Started && !replace => {
                        action.status = ActionStatus::Interrupted;
                        action.detail = Some(format!(
                            "a previous apply stopped mid-broadcast — check whether the call landed; if it did, set its status to `success` in {}/{network}.json, otherwise pass `--replace {name}` to broadcast it again",
                            runfile::RUN_DIR
                        ));
                    }
                    Some(record) if record.status == RecordStatus::Success => {
                        let changed = record.definition != definition;
                        let stale = stale_refs(step, record, &resolved, &lifecycles);
                        if replace {
                            action.action = "rerun";
                            action.detail = Some("already ran — runs again".to_string());
                        } else if !changed && stale.is_empty() {
                            action.status = ActionStatus::Complete;
                            if !record.args.is_empty() {
                                action.args = record.args.clone();
                            }
                        } else {
                            action.status = ActionStatus::Conflict;
                            action.detail = Some(conflict_detail(name, changed, &stale, true));
                        }
                    }
                    _ => action.action = "call",
                }
            }

            Step::Setting(setting) => {
                let target = resolved.eval(&setting.contract, height);
                let desired = resolved.eval(&setting.value, height);
                action.id = target.clone();
                action.args = vec![resolved.preview(&setting.value, height)];
                match (&target, &desired) {
                    (None, _) => {
                        action.action = "check";
                        action.detail = Some(
                            "checked once its contract exists — no transaction if the value already matches"
                                .to_string(),
                        );
                    }
                    (Some(target_id), desired) => {
                        let current = if setting.read.is_some() {
                            facts.readings.get(name).cloned()
                        } else {
                            recorded_setting_value(setting, target_id, run, height)
                                .map(Reading::Value)
                        };
                        match (current, desired) {
                            (Some(Reading::Failed(reason)), _) => {
                                action.status = ActionStatus::Blocked;
                                action.detail =
                                    Some(format!("cannot read the current value: {reason}"));
                            }
                            (Some(Reading::Value(current)), Some(desired))
                                if &current == desired =>
                            {
                                action.status = ActionStatus::Complete;
                            }
                            (current, _) => {
                                action.action = "update";
                                let from = match current {
                                    Some(Reading::Value(current)) => current,
                                    _ => "unknown".to_string(),
                                };
                                action.detail = Some(format!(
                                    "{from} → {}",
                                    resolved.preview(&setting.value, height)
                                ));
                            }
                        }
                    }
                }
            }
        }
        actions.push(action);
    }

    Ok(Reconciled { actions, resolved })
}

// ---------------------------------------------------------------------------
// Chain reads

fn decode_setting_value(
    value_type: ValueType,
    data_hex: &str,
) -> std::result::Result<String, String> {
    let bytes = hex::decode(data_hex.strip_prefix("0x").unwrap_or(data_hex))
        .map_err(|e| format!("the view returned non-hex data: {e}"))?;
    let le = |chunk: &[u8]| {
        let mut padded = [0_u8; 16];
        padded[..chunk.len()].copy_from_slice(chunk);
        u128::from_le_bytes(padded)
    };
    match value_type {
        ValueType::AlkaneId if bytes.len() == 32 => {
            Ok(format!("{}:{}", le(&bytes[..16]), le(&bytes[16..])))
        }
        ValueType::AlkaneId => Err(format!(
            "the view returned {} byte(s), expected a 32-byte Alkane id",
            bytes.len()
        )),
        ValueType::U128 if !bytes.is_empty() && bytes.len() <= 16 => Ok(le(&bytes).to_string()),
        ValueType::U128 => Err(format!(
            "the view returned {} byte(s), expected a u128",
            bytes.len()
        )),
    }
}

/// A setting's canonical value, so `2:07` and `2:7` compare equal.
fn normalize_setting_value(value_type: ValueType, value: &str) -> Result<String> {
    match value_type {
        ValueType::AlkaneId => {
            let (block, tx) = deployment::parse_id(value)?;
            Ok(format!("{block}:{tx}"))
        }
        ValueType::U128 => crate::parse_arg(value).map(|value| value.to_string()),
    }
}

async fn read_setting(
    config: &ToolkitConfig,
    setting: &Setting,
    target_id: &str,
) -> Result<Reading> {
    let read = setting.read.as_deref().expect("caller checked the read");
    let (block, tx) = deployment::parse_id(target_id)?;
    let opcode = match read.parse::<u128>() {
        Ok(opcode) => opcode,
        Err(_) => {
            let abi_bytes = match abi::fetch_deployed(config, block, tx).await {
                Ok(bytes) => bytes,
                Err(e) => return Ok(Reading::Failed(e.message)),
            };
            match abi::resolve_method(&abi_bytes, read, &CallArgs::Positional(Vec::new())) {
                Ok(method) => method.opcode,
                Err(e) => return Ok(Reading::Failed(e.message)),
            }
        }
    };
    let outcome = match toolkit::simulate(config, block, tx, opcode, &[]).await {
        Ok(outcome) => outcome,
        Err(e) => return Ok(Reading::Failed(e.message)),
    };
    if outcome.status != "success" {
        return Ok(Reading::Failed(
            outcome
                .error
                .unwrap_or_else(|| "the view reverted".to_string()),
        ));
    }
    Ok(
        match decode_setting_value(setting.value_type, &outcome.data) {
            Ok(value) => Reading::Value(value),
            Err(reason) => Reading::Failed(reason),
        },
    )
}

async fn gather_facts(config: &ToolkitConfig, graph: &Graph, known: &Resolved) -> Result<Facts> {
    use alkanes_cli_common::traits::BitcoinRpcProvider;

    let provider = crate::system::connect(config, None, false).await?;
    let chain_id = BitcoinRpcProvider::get_block_hash(&provider, 1).await.ok();
    let height = BitcoinRpcProvider::get_block_count(&provider)
        .await
        .map_err(|e| LabcoatError::classify(e.into()))?;

    let mut facts = Facts {
        chain_id,
        height,
        ..Facts::default()
    };
    for step in &graph.steps {
        match step {
            Step::Contract(contract) => {
                if let Some(number) = contract.reserve_number()? {
                    if let Ok(bytes) = abi::fetch_deployed(config, 4, number).await {
                        facts.reserve_abis.insert(number, bytes);
                    }
                }
            }
            Step::Setting(setting) if setting.read.is_some() => {
                if let Some(target) = known.eval(&setting.contract, height) {
                    let reading = read_setting(config, setting, &target).await?;
                    facts.readings.insert(setting.name.clone(), reading);
                }
            }
            _ => {}
        }
    }
    Ok(facts)
}

// ---------------------------------------------------------------------------
// Planning

/// Only a regtest chain is ever reset. On a shared network, a run file
/// written against a different chain means the endpoint is not the network
/// the records are for; ignoring them would deploy everything again.
fn refuse_foreign_chain(config: &ToolkitConfig, network: &str, stale: bool) -> Result<()> {
    if stale && !config.network.uses_regtest() {
        return Err(LabcoatError::new(
            "RUN_FILE_INVALID",
            format!(
                "{}/{network}.json was recorded against a different chain than the one this endpoint serves",
                runfile::RUN_DIR
            ),
            "check --network and --rpc-url; the run file's steps exist only on the chain it was recorded on",
        ));
    }
    Ok(())
}

/// Build the plan: reconcile the graph against the run file and chain
/// facts. Never loads a signer.
pub async fn plan(
    config: &ToolkitConfig,
    root: &Path,
    graph: &Graph,
    options: &Options,
) -> Result<PlanOutcome> {
    let network = config.network_id().to_string();
    let artifacts = resolve_artifacts(root, graph)?;
    let loaded = runfile::load(root, &network)?;
    let facts = gather_facts(config, graph, &known_ids(graph, &network, &loaded)).await?;

    // Records from a previous chain instance (before a reset) describe
    // contracts that no longer exist.
    let stale = loaded.chain_id.is_some() && loaded.chain_id != facts.chain_id;
    refuse_foreign_chain(config, &network, stale)?;
    let stale_records = if stale { loaded.steps.len() } else { 0 };
    let run = if stale {
        RunFile::empty(&network, facts.chain_id.clone())
    } else {
        loaded
    };
    // A stale run file invalidated the ids the reads were gathered with.
    let facts = if stale {
        gather_facts(config, graph, &known_ids(graph, &network, &run)).await?
    } else {
        facts
    };

    let Reconciled { actions, resolved } =
        reconcile(graph, &network, &artifacts, &facts, &run, options)?;

    let declared: BTreeSet<&str> = graph.steps.iter().map(Step::name).collect();
    let undeclared = run
        .steps
        .keys()
        .filter(|name| !declared.contains(name.as_str()))
        .cloned()
        .collect();
    let pending = actions
        .iter()
        .filter(|action| matches!(action.status, ActionStatus::Pending | ActionStatus::Adopt))
        .count();
    let conflicts = actions
        .iter()
        .filter(|action| action.status == ActionStatus::Conflict)
        .count();

    Ok(PlanOutcome {
        plan: Plan {
            script: deployment::SCRIPT.to_string(),
            network,
            chain_id: facts.chain_id,
            height: facts.height,
            actions,
            stale_records,
            undeclared,
            pending,
            conflicts,
        },
        artifacts,
        resolved,
        run,
    })
}

// ---------------------------------------------------------------------------
// Apply

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedAction {
    pub kind: &'static str,
    pub name: String,
    /// applied | adopted | in-sync | skipped-complete
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

fn stop(kind: &str, name: &str, error: LabcoatError) -> LabcoatError {
    LabcoatError::new(
        error.code,
        format!("apply stopped at {kind} `{name}`: {}", error.message),
        "completed steps are recorded; fix the cause and re-run apply to resume",
    )
}

fn reverted(what: &str, reason: Option<&str>) -> LabcoatError {
    LabcoatError::new(
        "EXECUTION_REVERT",
        format!("{what} reverted: {}", reason.unwrap_or("unknown reason")),
        "inspect the trace with `labcoat trace <txid>`",
    )
}

/// The ABI a contract step's code exposes: its local artifact, or — for a
/// clone — the deployed template's.
async fn contract_abi(
    config: &ToolkitConfig,
    contract: &Contract,
    artifacts: &BTreeMap<String, Artifact>,
    resolved: &Resolved,
    height: u64,
) -> Result<Option<Vec<u8>>> {
    match &contract.source {
        Source::Clone { of } => match resolved.eval(of, height) {
            Some(id) => {
                let (block, tx) = deployment::parse_id(&id)?;
                Ok(abi::fetch_deployed(config, block, tx).await.ok())
            }
            None => Ok(None),
        },
        _ => Ok(artifacts
            .get(&contract.name)
            .and_then(|artifact| artifact.abi.clone())),
    }
}

/// Execute the plan's pending steps in declaration order. Requires
/// broadcast intent — the CLI only calls this under `--broadcast`.
pub async fn apply(
    config: &ToolkitConfig,
    signer: &crate::signer::SignerSpec,
    root: &Path,
    graph: &Graph,
    options: &Options,
) -> Result<ApplyReport> {
    let PlanOutcome {
        plan,
        artifacts,
        mut resolved,
        mut run,
    } = plan(config, root, graph, options).await?;

    // Fail fast on states that need a human, before any broadcast.
    let conflicts: Vec<String> = plan
        .actions
        .iter()
        .filter(|action| action.status == ActionStatus::Conflict)
        .map(|action| {
            format!(
                "{} `{}`: {}",
                action.kind,
                action.name,
                action.detail.as_deref().unwrap_or("its definition changed")
            )
        })
        .collect();
    if !conflicts.is_empty() {
        return Err(LabcoatError::new(
            "DEPLOYMENT_CONFLICT",
            format!(
                "{} step(s) no longer match what ran on {}: {}",
                conflicts.len(),
                plan.network,
                conflicts.join(" | ")
            ),
            "a step that ran is never changed in place — restore it in deploy.rs, or name it in `--replace` to run it again; on Labcoat Network, `labcoat reset` starts a fresh chain",
        ));
    }
    for action in &plan.actions {
        if matches!(
            action.status,
            ActionStatus::Interrupted | ActionStatus::Blocked
        ) {
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
    }

    let network = plan.network.clone();
    let height = plan.height;
    run.chain_id = plan.chain_id.clone();
    let mut report = ApplyReport {
        network: network.clone(),
        chain_id: plan.chain_id.clone(),
        actions: Vec::new(),
    };
    let skipped = |action: &PlannedAction| AppliedAction {
        kind: action.kind,
        name: action.name.clone(),
        outcome: "skipped-complete".into(),
        id: action.id.clone(),
        txid: None,
        fee: None,
        detail: None,
    };

    for (step, action) in graph.steps.iter().zip(&plan.actions) {
        let name = step.name();
        match step {
            Step::External(_) => {
                // Keep the run file a complete address book.
                let id = action.id.clone();
                if record_of(&run, name, "external").and_then(|r| r.id.clone()) != id {
                    let mut record = StepRecord::new(
                        "external",
                        RecordStatus::Success,
                        definition_hash(step, None),
                    );
                    record.id = id;
                    run.steps.insert(name.to_string(), record);
                    runfile::save(root, &run)?;
                }
            }

            Step::Contract(contract) => {
                if action.status == ActionStatus::Complete {
                    report.actions.push(skipped(action));
                    continue;
                }
                let artifact = artifacts.get(name);
                let wasm_sha256 = artifact.map(|artifact| artifact.wasm_sha256.clone());
                let definition = definition_hash(step, wasm_sha256.as_deref());
                let previous = record_of(&run, name, "contract").cloned();
                let mut record = StepRecord::new("contract", RecordStatus::Started, definition)
                    .replacing(previous.as_ref());
                record.wasm_sha256 = wasm_sha256.clone();
                record.artifact = artifact.and_then(Artifact::stem);
                record.height = Some(height);
                record.refs = current_refs(step, &resolved);

                if action.status == ActionStatus::Adopt {
                    let id = action.id.clone().expect("adopted reserves have an id");
                    // Durable-state guard before the adoption is recorded:
                    // a reset or foreign chain aborts this step, and the
                    // lease covers both ledger writes.
                    let mut state_guard = crate::state::deploy_guard(
                        root,
                        &config.environment,
                        &crate::state::observed_chain(config, plan.chain_id.clone()),
                    )
                    .map_err(|e| stop("contract", name, e))?;
                    let deployment = lockfile::Deployment {
                        alkanes_id: id.clone(),
                        wasm_sha256,
                        txid: "adopted".into(),
                        block: None,
                        status: "success".into(),
                        deployed_at: runfile::now_millis(),
                        chain_id: plan.chain_id.clone(),
                    };
                    lockfile::record(root, &network, name, deployment.clone())
                        .map_err(|e| stop("contract", name, e))?;
                    if let Some((mut lease, v2_state)) = state_guard.take() {
                        if let Err(e) = crate::state::record_instance(
                            &mut lease,
                            v2_state,
                            name,
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
                    record.status = RecordStatus::Success;
                    record.id = Some(id.clone());
                    record.created = vec![id.clone()];
                    record.origin = Some("adopted".into());
                    run.steps.insert(name.to_string(), record);
                    runfile::save(root, &run)?;
                    resolved
                        .insert(name, &id, std::slice::from_ref(&id))
                        .map_err(|e| stop("contract", name, e))?;
                    report.actions.push(AppliedAction {
                        kind: "contract",
                        name: name.to_string(),
                        outcome: "adopted".into(),
                        id: Some(id),
                        txid: None,
                        fee: None,
                        detail: action.detail.clone(),
                    });
                    continue;
                }

                let fail = |e: LabcoatError| stop("contract", name, e);
                let target = deploy_target(contract, &resolved, height)
                    .map_err(fail)?
                    .ok_or_else(|| fail(unresolved("contract", name)))?;
                let args = resolve_args(&contract.init.args, &resolved, height)
                    .ok_or_else(|| fail(unresolved("contract", name)))?;
                let tx_options = resolve_tx(&contract.tx, &resolved, height)
                    .ok_or_else(|| fail(unresolved("contract", name)))?;
                let is_clone = matches!(target, DeployTarget::Clone { .. });
                let (initializer, cellpack_args) = match contract.init.method.as_deref() {
                    Some(method) => {
                        let abi_bytes =
                            contract_abi(config, contract, &artifacts, &resolved, height)
                                .await
                                .map_err(fail)?;
                        let method = abi::encode_invocation(abi_bytes.as_deref(), method, &args)
                            .map_err(fail)?;
                        (Some(method.opcode), method.cellpack_args)
                    }
                    None if is_clone && args.is_empty() => (None, Vec::new()),
                    None if is_clone => {
                        let abi_bytes =
                            contract_abi(config, contract, &artifacts, &resolved, height)
                                .await
                                .map_err(fail)?;
                        let method = abi_bytes
                            .as_deref()
                            .map(|bytes| abi::resolve_constructor(bytes, &args))
                            .transpose()
                            .map_err(fail)?
                            .flatten()
                            .ok_or_else(|| {
                                fail(LabcoatError::new(
                                    "CONFIG_INVALID",
                                    "the template exposes no opcode-0 constructor for these args",
                                    "name the initializer with `.init(method)`",
                                ))
                            })?;
                        (Some(0), method.cellpack_args)
                    }
                    None => {
                        let wasm_path = &artifact
                            .expect("non-clone contracts have an artifact")
                            .wasm_path;
                        let constructor =
                            abi::encode_constructor(wasm_path, &args).map_err(fail)?;
                        (None, constructor.cellpack)
                    }
                };

                record.args = display_args(&args);
                run.steps.insert(name.to_string(), record.clone());
                runfile::save(root, &run)?;

                let no_wasm = PathBuf::new();
                let outcome = toolkit::deploy_in(
                    config,
                    signer,
                    root,
                    toolkit::DeployRequest {
                        wasm_path: artifact
                            .map(|artifact| artifact.wasm_path.as_path())
                            .unwrap_or(&no_wasm),
                        contract_name: Some(name.to_string()),
                        initializer,
                        cellpack_args: &cellpack_args,
                        fee_rate: config.fee_rate,
                        target,
                        options: &tx_options,
                    },
                )
                .await
                // The `started` record stays: the broadcast may have landed.
                .map_err(fail)?;

                record.txid = Some(outcome.txid.clone());
                record.updated_at = runfile::now_millis();
                let id = match (&outcome.alkanes_id, outcome.status == "success") {
                    (Some(id), true) => id.clone(),
                    _ => {
                        record.status = RecordStatus::Revert;
                        run.steps.insert(name.to_string(), record);
                        runfile::save(root, &run)?;
                        return Err(fail(reverted(
                            "deployment",
                            outcome.revert_reason.as_deref(),
                        )));
                    }
                };
                record.status = RecordStatus::Success;
                record.id = Some(id.clone());
                record.created = if outcome.created.first() == Some(&id) {
                    outcome.created.clone()
                } else {
                    std::iter::once(id.clone())
                        .chain(
                            outcome
                                .created
                                .iter()
                                .filter(|other| **other != id)
                                .cloned(),
                        )
                        .collect()
                };
                record.origin = Some(if is_clone { "cloned" } else { "deployed" }.into());
                resolved.insert(name, &id, &record.created).map_err(fail)?;
                run.steps.insert(name.to_string(), record);
                runfile::save(root, &run)?;
                report.actions.push(AppliedAction {
                    kind: "contract",
                    name: name.to_string(),
                    outcome: "applied".into(),
                    id: Some(id),
                    txid: Some(outcome.txid.clone()),
                    fee: Some(outcome.fee),
                    detail: action.detail.clone(),
                });
            }

            Step::Call(call) => {
                if action.status == ActionStatus::Complete {
                    report.actions.push(skipped(action));
                    continue;
                }
                let fail = |e: LabcoatError| stop("call", name, e);
                let target_id = resolved
                    .eval(&call.contract, height)
                    .ok_or_else(|| fail(unresolved("call", name)))?;
                let args = resolve_args(&call.call.args, &resolved, height)
                    .ok_or_else(|| fail(unresolved("call", name)))?;
                let tx_options = resolve_tx(&call.tx, &resolved, height)
                    .ok_or_else(|| fail(unresolved("call", name)))?;
                let (block, tx) = deployment::parse_id(&target_id).map_err(fail)?;
                let method_name = call.call.method.as_deref().unwrap_or_default();

                let abi_bytes = if method_name.parse::<u128>().is_ok() {
                    None
                } else {
                    Some(match call.call.abi.as_deref() {
                        Some(abi_step) => {
                            let Some(Step::Contract(owner)) =
                                graph.steps.iter().find(|step| step.name() == abi_step)
                            else {
                                unreachable!("validated: abi names a contract step");
                            };
                            contract_abi(config, owner, &artifacts, &resolved, height)
                                .await
                                .map_err(fail)?
                                .ok_or_else(|| {
                                    fail(LabcoatError::new(
                                        "CONFIG_INVALID",
                                        format!("contract `{abi_step}` exposes no ABI metadata"),
                                        "export __meta from the contract, or call a numeric opcode",
                                    ))
                                })?
                        }
                        None => abi::fetch_deployed(config, block, tx).await.map_err(fail)?,
                    })
                };
                let method = abi::encode_invocation(abi_bytes.as_deref(), method_name, &args)
                    .map_err(fail)?;

                // Record the attempt before broadcasting, so a crash
                // between broadcast and confirmation shows as `started`
                // instead of silently running again.
                let previous = record_of(&run, name, "call").cloned();
                let mut record =
                    StepRecord::new("call", RecordStatus::Started, definition_hash(step, None))
                        .replacing(previous.as_ref());
                record.id = Some(target_id.clone());
                record.refs = current_refs(step, &resolved);
                record.args = display_args(&args);
                record.height = Some(height);
                run.steps.insert(name.to_string(), record.clone());
                runfile::save(root, &run)?;

                let outcome = toolkit::call(
                    config,
                    signer,
                    toolkit::CallRequest {
                        block,
                        tx,
                        opcode: method.opcode,
                        args: &method.cellpack_args,
                        fee_rate: config.fee_rate,
                        options: &tx_options,
                    },
                )
                .await
                .map_err(fail)?;

                let succeeded = outcome.status == "success";
                record.status = if succeeded {
                    RecordStatus::Success
                } else {
                    RecordStatus::Revert
                };
                record.txid = Some(outcome.txid.clone());
                record.updated_at = runfile::now_millis();
                run.steps.insert(name.to_string(), record);
                runfile::save(root, &run)?;
                if !succeeded {
                    return Err(fail(reverted("call", outcome.revert_reason.as_deref())));
                }
                report.actions.push(AppliedAction {
                    kind: "call",
                    name: name.to_string(),
                    outcome: "applied".into(),
                    id: Some(target_id),
                    txid: Some(outcome.txid.clone()),
                    fee: Some(outcome.fee),
                    detail: None,
                });
            }

            Step::Setting(setting) => {
                if action.status == ActionStatus::Complete
                    && record_of(&run, name, "setting").is_some()
                {
                    report.actions.push(skipped(action));
                    continue;
                }
                let fail = |e: LabcoatError| stop("setting", name, e);
                let target_id = resolved
                    .eval(&setting.contract, height)
                    .ok_or_else(|| fail(unresolved("setting", name)))?;
                let desired = resolved
                    .eval(&setting.value, height)
                    .ok_or_else(|| fail(unresolved("setting", name)))
                    .and_then(|value| {
                        normalize_setting_value(setting.value_type, &value).map_err(fail)
                    })?;
                let (block, tx) = deployment::parse_id(&target_id).map_err(fail)?;

                // Ids are final now; read again before deciding to write.
                let current = if setting.read.is_some() {
                    match read_setting(config, setting, &target_id)
                        .await
                        .map_err(fail)?
                    {
                        Reading::Value(value) => Some(value),
                        Reading::Failed(reason) => {
                            return Err(fail(LabcoatError::new(
                                "APPLY_BLOCKED",
                                format!("cannot read the current value: {reason}"),
                                "check the setting's read method",
                            )))
                        }
                    }
                } else {
                    recorded_setting_value(setting, &target_id, &run, height)
                };

                let previous = record_of(&run, name, "setting").cloned();
                let mut record = StepRecord::new(
                    "setting",
                    RecordStatus::Success,
                    definition_hash(step, None),
                );
                record.id = Some(target_id.clone());
                record.refs = current_refs(step, &resolved);
                record.height = Some(height);
                record.value = Some(desired.clone());

                if current.as_deref() == Some(desired.as_str()) {
                    // Nothing to write; remember what was observed.
                    if let Some(previous) = &previous {
                        record.history = previous.history.clone();
                        record.txid = previous.txid.clone();
                    }
                    run.steps.insert(name.to_string(), record);
                    runfile::save(root, &run)?;
                    report.actions.push(AppliedAction {
                        kind: "setting",
                        name: name.to_string(),
                        outcome: "in-sync".into(),
                        id: Some(target_id),
                        txid: None,
                        fee: None,
                        detail: Some(format!("already {desired}")),
                    });
                    continue;
                }

                let tx_options = resolve_tx(&setting.tx, &resolved, height)
                    .ok_or_else(|| fail(unresolved("setting", name)))?;
                let write = setting.write.as_deref().unwrap_or_default();
                let method = if let Ok(opcode) = write.parse::<u128>() {
                    let cellpack_args = match setting.value_type {
                        ValueType::AlkaneId => {
                            let (b, t) = deployment::parse_id(&desired).map_err(fail)?;
                            vec![b, t]
                        }
                        ValueType::U128 => vec![crate::parse_arg(&desired).map_err(fail)?],
                    };
                    abi::ResolvedMethod {
                        name: write.to_string(),
                        opcode,
                        cellpack_args,
                    }
                } else {
                    let abi_bytes = abi::fetch_deployed(config, block, tx).await.map_err(fail)?;
                    abi::resolve_method(
                        &abi_bytes,
                        write,
                        &CallArgs::Positional(vec![desired.clone()]),
                    )
                    .map_err(fail)?
                };

                let mut started = record.clone().replacing(previous.as_ref());
                started.status = RecordStatus::Started;
                run.steps.insert(name.to_string(), started.clone());
                runfile::save(root, &run)?;

                let outcome = toolkit::call(
                    config,
                    signer,
                    toolkit::CallRequest {
                        block,
                        tx,
                        opcode: method.opcode,
                        args: &method.cellpack_args,
                        fee_rate: config.fee_rate,
                        options: &tx_options,
                    },
                )
                .await
                .map_err(fail)?;

                started.txid = Some(outcome.txid.clone());
                started.updated_at = runfile::now_millis();
                if outcome.status != "success" {
                    started.status = RecordStatus::Revert;
                    started.value = current.clone();
                    run.steps.insert(name.to_string(), started);
                    runfile::save(root, &run)?;
                    return Err(fail(reverted("update", outcome.revert_reason.as_deref())));
                }
                // Read the value back: a write that succeeds without
                // taking effect must not be recorded as applied.
                if setting.read.is_some() {
                    let after = read_setting(config, setting, &target_id)
                        .await
                        .map_err(fail)?;
                    if after != Reading::Value(desired.clone()) {
                        return Err(fail(LabcoatError::new(
                            "APPLY_BLOCKED",
                            format!(
                                "the update transaction {} succeeded but the value reads back as {}",
                                outcome.txid,
                                match after {
                                    Reading::Value(value) => value,
                                    Reading::Failed(reason) => format!("unreadable ({reason})"),
                                }
                            ),
                            "check the setting's read and write methods",
                        )));
                    }
                }
                started.status = RecordStatus::Success;
                run.steps.insert(name.to_string(), started);
                runfile::save(root, &run)?;
                report.actions.push(AppliedAction {
                    kind: "setting",
                    name: name.to_string(),
                    outcome: "applied".into(),
                    id: Some(target_id),
                    txid: Some(outcome.txid.clone()),
                    fee: Some(outcome.fee),
                    detail: Some(format!(
                        "{} → {desired}",
                        current.unwrap_or_else(|| "unknown".to_string())
                    )),
                });
            }
        }
    }

    Ok(report)
}

// ---------------------------------------------------------------------------
// Import

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub network: String,
    pub name: String,
    pub id: String,
    /// Whether the deployed ABI was compared with the local artifact.
    pub abi_verified: bool,
}

/// Record a contract that already exists on chain as a step's instance,
/// without deploying: for deployments made before the script existed, or
/// to resolve an interrupted deploy that did land.
pub async fn import(
    config: &ToolkitConfig,
    root: &Path,
    graph: &Graph,
    name: &str,
    id: &str,
) -> Result<ImportReport> {
    let network = config.network_id().to_string();
    let Some(step) = graph.steps.iter().find(|step| step.name() == name) else {
        return Err(LabcoatError::new(
            "CONFIG_INVALID",
            format!("deploy.rs declares no step named `{name}`"),
            "run `labcoat plan` to list the declared steps",
        ));
    };
    if !matches!(step, Step::Contract(_)) {
        return Err(LabcoatError::new(
            "CONFIG_INVALID",
            format!(
                "`{name}` is a {} step — only contracts can be imported",
                step.kind()
            ),
            "declare an Alkane you do not deploy with `d.external(name)` instead",
        ));
    }
    let (block, tx) = deployment::parse_id(id).map_err(|_| {
        LabcoatError::new(
            "CONFIG_INVALID",
            format!("`{id}` is not a block:tx Alkane id"),
            "pass the deployed contract's id, for example 2:7",
        )
    })?;
    let id = format!("{block}:{tx}");

    let artifacts = resolve_artifacts(root, graph)?;
    let loaded = runfile::load(root, &network)?;
    let chain_id = toolkit::chain_instance_id(config).await;
    let stale = loaded.chain_id.is_some() && loaded.chain_id != chain_id;
    refuse_foreign_chain(config, &network, stale)?;
    let mut run = if stale {
        RunFile::empty(&network, chain_id.clone())
    } else {
        loaded
    };
    run.chain_id = chain_id.clone();
    if let Some(existing) = success_record(&run, name, "contract") {
        return Err(LabcoatError::new(
            "CONFIG_INVALID",
            format!(
                "`{name}` is already recorded as {} on {network}",
                existing.id.as_deref().unwrap_or("an unknown id")
            ),
            "to deploy a new instance instead, run `labcoat apply --replace <name> --broadcast`",
        ));
    }

    let onchain = abi::fetch_deployed(config, block, tx).await.map_err(|e| {
        LabcoatError::new(
            "CONTRACT_NOT_FOUND",
            format!("cannot read a contract at {id} on {network}: {}", e.message),
            "check the id and that the indexer is caught up",
        )
    })?;
    let artifact = artifacts.get(name);
    let abi_verified = match artifact.and_then(|artifact| artifact.abi.as_deref()) {
        Some(local) => {
            if !abi::compare(local, &onchain).matches {
                return Err(LabcoatError::new(
                    "CONFIG_INVALID",
                    format!(
                        "the contract at {id} exposes a different ABI than `{name}`'s local build"
                    ),
                    "import the id of a deployment of this contract's code",
                ));
            }
            true
        }
        None => false,
    };

    let resolved = known_ids(graph, &network, &run);
    let wasm_sha256 = artifact.map(|artifact| artifact.wasm_sha256.clone());
    let previous = record_of(&run, name, "contract").cloned();
    let mut record = StepRecord::new(
        "contract",
        RecordStatus::Success,
        definition_hash(step, wasm_sha256.as_deref()),
    )
    .replacing(previous.as_ref());
    record.id = Some(id.clone());
    record.created = vec![id.clone()];
    record.refs = current_refs(step, &resolved);
    record.wasm_sha256 = wasm_sha256.clone();
    record.artifact = artifact.and_then(Artifact::stem);
    record.origin = Some("imported".into());
    run.steps.insert(name.to_string(), record);
    runfile::save(root, &run)?;
    lockfile::record(
        root,
        &network,
        name,
        lockfile::Deployment {
            alkanes_id: id.clone(),
            wasm_sha256,
            txid: "imported".into(),
            block: None,
            status: "success".into(),
            deployed_at: runfile::now_millis(),
            chain_id,
        },
    )?;

    Ok(ImportReport {
        network,
        name: name.to_string(),
        id,
        abi_verified,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deployment::GRAPH_SCHEMA;

    const ABI: &str = r#"{"contract":"T","methods":[{"name":"initialize","opcode":0,"params":[],"returns":"void"}]}"#;

    fn graph(steps: &str) -> Graph {
        deployment::parse(
            &format!(r#"{{"schema":"{GRAPH_SCHEMA}","network":"labcoat","steps":[{steps}]}}"#),
            "labcoat",
        )
        .unwrap()
    }

    fn contract(name: &str, lifecycle: &str, extra: &str, args: &str) -> String {
        format!(
            r#"{{"kind":"contract","name":"{name}","source":{{"type":"package","package":"{name}"}},"lifecycle":"{lifecycle}","init":{{"args":{{"named":[{args}]}}}},"tx":{{}}{extra}}}"#
        )
    }

    fn id_ref(step: &str) -> String {
        format!(r#"{{"ref":{{"step":"{step}","field":"id"}}}}"#)
    }

    fn artifacts(entries: &[(&str, &str)]) -> BTreeMap<String, Artifact> {
        entries
            .iter()
            .map(|(name, sha)| {
                (
                    name.to_string(),
                    Artifact {
                        wasm_path: PathBuf::from(format!("build/{name}.wasm")),
                        wasm_sha256: sha.to_string(),
                        abi: Some(ABI.as_bytes().to_vec()),
                    },
                )
            })
            .collect()
    }

    fn facts() -> Facts {
        Facts {
            chain_id: Some("chain".into()),
            height: 200,
            ..Facts::default()
        }
    }

    /// Record every pending contract and call as if apply had run it,
    /// assigning sequential ids from `next`.
    fn settle(
        graph: &Graph,
        artifacts: &BTreeMap<String, Artifact>,
        run: &mut RunFile,
        options: &Options,
        next: &mut u128,
    ) {
        let Reconciled {
            actions,
            mut resolved,
        } = reconcile(graph, "labcoat", artifacts, &facts(), run, options).unwrap();
        for (step, action) in graph.steps.iter().zip(&actions) {
            if action.status != ActionStatus::Pending {
                continue;
            }
            let sha = artifacts.get(step.name()).map(|a| a.wasm_sha256.as_str());
            let previous = run.steps.get(step.name()).cloned();
            let mut record = StepRecord::new(
                step.kind(),
                RecordStatus::Success,
                definition_hash(step, sha),
            )
            .replacing(previous.as_ref());
            record.refs = current_refs(step, &resolved);
            if let Step::Contract(_) = step {
                let id = action.id.clone().unwrap_or_else(|| {
                    *next += 1;
                    format!("2:{next}")
                });
                resolved
                    .insert(step.name(), &id, std::slice::from_ref(&id))
                    .unwrap();
                record.id = Some(id.clone());
                record.created = vec![id];
            }
            run.steps.insert(step.name().to_string(), record);
        }
    }

    fn statuses(
        graph: &Graph,
        artifacts: &BTreeMap<String, Artifact>,
        run: &RunFile,
        options: &Options,
    ) -> Vec<(String, ActionStatus, &'static str)> {
        reconcile(graph, "labcoat", artifacts, &facts(), run, options)
            .unwrap()
            .actions
            .into_iter()
            .map(|action| (action.name, action.status, action.action))
            .collect()
    }

    fn replace(names: &[&str]) -> Options {
        Options {
            replace: names.iter().map(|name| name.to_string()).collect(),
        }
    }

    fn token_and_market() -> Graph {
        graph(&format!(
            "{},{}",
            contract("token", "pinned", "", r#"["supply",{"lit":"100"}]"#),
            contract(
                "market",
                "pinned",
                "",
                &format!(
                    r#"["base",{}],["expiry",{{"height":{{"offset":100}}}}]"#,
                    id_ref("token")
                )
            ),
        ))
    }

    #[test]
    fn a_fresh_graph_is_pending_and_an_applied_one_is_complete() {
        let graph = token_and_market();
        let artifacts = artifacts(&[("token", "aa"), ("market", "bb")]);
        let mut run = RunFile::empty("labcoat", Some("chain".into()));
        assert_eq!(
            statuses(&graph, &artifacts, &run, &Options::default()),
            [
                ("token".to_string(), ActionStatus::Pending, "create"),
                ("market".to_string(), ActionStatus::Pending, "create"),
            ]
        );

        settle(&graph, &artifacts, &mut run, &Options::default(), &mut 4);
        assert!(statuses(&graph, &artifacts, &run, &Options::default())
            .iter()
            .all(|(_, status, _)| *status == ActionStatus::Complete));
    }

    #[test]
    fn height_never_makes_a_recorded_step_conflict() {
        let graph = token_and_market();
        let artifacts = artifacts(&[("token", "aa"), ("market", "bb")]);
        let mut run = RunFile::empty("labcoat", Some("chain".into()));
        settle(&graph, &artifacts, &mut run, &Options::default(), &mut 4);

        let later = Facts {
            height: 9_999,
            ..facts()
        };
        let actions = reconcile(
            &graph,
            "labcoat",
            &artifacts,
            &later,
            &run,
            &Options::default(),
        )
        .unwrap()
        .actions;
        assert!(actions
            .iter()
            .all(|action| action.status == ActionStatus::Complete));
    }

    #[test]
    fn changed_code_or_args_on_a_pinned_contract_is_a_conflict() {
        let graph = token_and_market();
        let mut run = RunFile::empty("labcoat", Some("chain".into()));
        settle(
            &graph,
            &artifacts(&[("token", "aa"), ("market", "bb")]),
            &mut run,
            &Options::default(),
            &mut 4,
        );

        // New wasm for the token.
        let rebuilt = artifacts(&[("token", "cc"), ("market", "bb")]);
        let planned = statuses(&graph, &rebuilt, &run, &Options::default());
        assert_eq!(planned[0].1, ActionStatus::Conflict);
        assert_eq!(planned[1].1, ActionStatus::Complete);

        // New constructor arg, same wasm.
        let edited = self::graph(&format!(
            "{},{}",
            contract("token", "pinned", "", r#"["supply",{"lit":"1000"}]"#),
            contract(
                "market",
                "pinned",
                "",
                &format!(r#"["base",{}]"#, id_ref("token"))
            ),
        ));
        let planned = statuses(
            &edited,
            &artifacts(&[("token", "aa"), ("market", "bb")]),
            &run,
            &Options::default(),
        );
        assert_eq!(planned[0].1, ActionStatus::Conflict);
        assert_eq!(
            planned[1].1,
            ActionStatus::Conflict,
            "market's args changed too"
        );
    }

    #[test]
    fn replacing_a_contract_requires_replacing_what_references_it() {
        let graph = token_and_market();
        let artifacts = artifacts(&[("token", "aa"), ("market", "bb")]);
        let mut run = RunFile::empty("labcoat", Some("chain".into()));
        let mut next = 4;
        settle(&graph, &artifacts, &mut run, &Options::default(), &mut next);

        let planned = statuses(&graph, &artifacts, &run, &replace(&["token"]));
        assert_eq!(
            planned[0],
            ("token".to_string(), ActionStatus::Pending, "replace")
        );
        assert_eq!(planned[1].1, ActionStatus::Conflict);

        let both = replace(&["token", "market"]);
        let planned = statuses(&graph, &artifacts, &run, &both);
        assert!(planned
            .iter()
            .all(|(_, status, action)| *status == ActionStatus::Pending && *action == "replace"));

        settle(&graph, &artifacts, &mut run, &both, &mut next);
        assert_eq!(run.steps["token"].id.as_deref(), Some("2:7"));
        assert_eq!(run.steps["token"].history[0].id.as_deref(), Some("2:5"));
        assert_eq!(run.steps["market"].refs["token"], "2:7");
        assert!(statuses(&graph, &artifacts, &run, &Options::default())
            .iter()
            .all(|(_, status, _)| *status == ActionStatus::Complete));
    }

    #[test]
    fn a_replaceable_implementation_redeploys_without_disturbing_its_consumers() {
        let graph = graph(&format!(
            "{},{}",
            contract("logic", "replaceable", "", ""),
            contract(
                "beacon",
                "pinned",
                "",
                &format!(r#"["implementation",{}]"#, id_ref("logic"))
            ),
        ));
        let mut run = RunFile::empty("labcoat", Some("chain".into()));
        let mut next = 4;
        settle(
            &graph,
            &artifacts(&[("logic", "v1"), ("beacon", "bb")]),
            &mut run,
            &Options::default(),
            &mut next,
        );

        let rebuilt = artifacts(&[("logic", "v2"), ("beacon", "bb")]);
        let planned = statuses(&graph, &rebuilt, &run, &Options::default());
        assert_eq!(
            planned[0],
            ("logic".to_string(), ActionStatus::Pending, "replace")
        );
        assert_eq!(planned[1].1, ActionStatus::Complete);

        settle(&graph, &rebuilt, &mut run, &Options::default(), &mut next);
        assert_eq!(run.steps["logic"].id.as_deref(), Some("2:7"));
        assert_eq!(run.steps["logic"].history.len(), 1);
        assert!(statuses(&graph, &rebuilt, &run, &Options::default())
            .iter()
            .all(|(_, status, _)| *status == ActionStatus::Complete));
    }

    #[test]
    fn a_changed_external_id_conflicts_with_what_consumed_it() {
        let steps = |tx: u32| {
            format!(
                r#"{{"kind":"external","name":"usd","ids":{{"*":[4,{tx}]}}}},{}"#,
                contract(
                    "market",
                    "pinned",
                    "",
                    &format!(r#"["quote",{}]"#, id_ref("usd"))
                )
            )
        };
        let artifacts = artifacts(&[("market", "bb")]);
        let mut run = RunFile::empty("labcoat", Some("chain".into()));
        settle(
            &graph(&steps(1)),
            &artifacts,
            &mut run,
            &Options::default(),
            &mut 4,
        );

        let planned = statuses(&graph(&steps(2)), &artifacts, &run, &Options::default());
        assert_eq!(planned[0].1, ActionStatus::Complete);
        assert_eq!(planned[1].1, ActionStatus::Conflict);
    }

    #[test]
    fn an_unbound_external_fails_the_plan() {
        let graph = graph(r#"{"kind":"external","name":"usd","ids":{"signet":[2,5]}}"#);
        let error = reconcile(
            &graph,
            "labcoat",
            &BTreeMap::new(),
            &facts(),
            &RunFile::empty("labcoat", None),
            &Options::default(),
        )
        .err()
        .unwrap();
        assert_eq!(error.code, "CONFIG_INVALID");
        assert!(error.message.contains("no id bound for network `labcoat`"));
    }

    #[test]
    fn reserves_resolve_before_apply_and_occupied_ones_adopt_or_block() {
        let graph = graph(&format!(
            "{},{}",
            contract("token", "pinned", r#","reserve":"65011""#, ""),
            contract(
                "market",
                "pinned",
                "",
                &format!(r#"["base",{}]"#, id_ref("token"))
            ),
        ));
        let artifacts = artifacts(&[("token", "aa"), ("market", "bb")]);
        let run = RunFile::empty("labcoat", Some("chain".into()));

        let actions = reconcile(
            &graph,
            "labcoat",
            &artifacts,
            &facts(),
            &run,
            &Options::default(),
        )
        .unwrap()
        .actions;
        assert_eq!(actions[0].id.as_deref(), Some("4:65011"));
        assert_eq!(actions[0].target.as_deref(), Some("3:65011"));
        assert_eq!(actions[1].args, ["base=4:65011"]);

        let mut occupied = facts();
        occupied.reserve_abis.insert(65011, ABI.as_bytes().to_vec());
        let actions = reconcile(
            &graph,
            "labcoat",
            &artifacts,
            &occupied,
            &run,
            &Options::default(),
        )
        .unwrap()
        .actions;
        assert_eq!(actions[0].status, ActionStatus::Adopt);

        occupied
            .reserve_abis
            .insert(65011, br#"{"contract":"Other","methods":[]}"#.to_vec());
        let actions = reconcile(
            &graph,
            "labcoat",
            &artifacts,
            &occupied,
            &run,
            &Options::default(),
        )
        .unwrap()
        .actions;
        assert_eq!(actions[0].status, ActionStatus::Blocked);
    }

    #[test]
    fn a_started_record_is_interrupted_until_replaced() {
        let graph = token_and_market();
        let artifacts = artifacts(&[("token", "aa"), ("market", "bb")]);
        let mut run = RunFile::empty("labcoat", Some("chain".into()));
        run.steps.insert(
            "token".into(),
            StepRecord::new("contract", RecordStatus::Started, "x".into()),
        );
        let planned = statuses(&graph, &artifacts, &run, &Options::default());
        assert_eq!(planned[0].1, ActionStatus::Interrupted);
        let planned = statuses(&graph, &artifacts, &run, &replace(&["token"]));
        assert_eq!(
            planned[0],
            ("token".to_string(), ActionStatus::Pending, "create")
        );
    }

    fn beacon_graph() -> Graph {
        graph(&format!(
            r#"{},{},{{"kind":"setting","name":"beacon_implementation","contract":{},"value":{},"valueType":"alkane-id","read":"implementation","write":"upgrade_to","tx":{{}}}}"#,
            contract("logic", "replaceable", "", ""),
            contract(
                "beacon",
                "pinned",
                "",
                &format!(r#"["implementation",{}]"#, id_ref("logic"))
            ),
            id_ref("beacon"),
            id_ref("logic"),
        ))
    }

    #[test]
    fn a_setting_converges_on_what_the_chain_reads() {
        let graph = beacon_graph();
        let mut run = RunFile::empty("labcoat", Some("chain".into()));
        let mut next = 4;
        let v1 = artifacts(&[("logic", "v1"), ("beacon", "bb")]);

        // Before anything exists the setting is deferred.
        let planned = statuses(&graph, &v1, &run, &Options::default());
        assert_eq!(
            planned[2],
            (
                "beacon_implementation".to_string(),
                ActionStatus::Pending,
                "check"
            )
        );

        settle(&graph, &v1, &mut run, &Options::default(), &mut next);
        let read = |value: &str| {
            let mut facts = facts();
            facts
                .readings
                .insert("beacon_implementation".into(), Reading::Value(value.into()));
            facts
        };
        let status = |facts: &Facts, artifacts: &BTreeMap<String, Artifact>, run: &RunFile| {
            let actions = reconcile(
                &graph,
                "labcoat",
                artifacts,
                facts,
                run,
                &Options::default(),
            )
            .unwrap()
            .actions;
            (actions[2].status, actions[2].detail.clone())
        };

        // In sync: the beacon points at logic 2:5.
        assert_eq!(status(&read("2:5"), &v1, &run).0, ActionStatus::Complete);
        // Drift: someone pointed the beacon elsewhere.
        let (drifted, detail) = status(&read("2:99"), &v1, &run);
        assert_eq!(drifted, ActionStatus::Pending);
        assert_eq!(detail.as_deref(), Some("2:99 → 2:5"));
        // New code: the implementation is replaced, so the target is not
        // known until apply.
        let v2 = artifacts(&[("logic", "v2"), ("beacon", "bb")]);
        let (upgrade, detail) = status(&read("2:5"), &v2, &run);
        assert_eq!(upgrade, ActionStatus::Pending);
        assert_eq!(detail.as_deref(), Some("2:5 → <logic.id>"));
        // An unreadable value blocks rather than guessing.
        let mut failed = facts();
        failed.readings.insert(
            "beacon_implementation".into(),
            Reading::Failed("boom".into()),
        );
        assert_eq!(status(&failed, &v1, &run).0, ActionStatus::Blocked);
    }

    #[test]
    fn a_read_less_setting_starts_from_what_the_initializer_was_given() {
        let graph = graph(&format!(
            r#"{},{},{{"kind":"setting","name":"proxy_implementation","contract":{},"value":{},"valueType":"alkane-id","write":"upgrade","tx":{{}}}}"#,
            contract("logic", "replaceable", "", ""),
            contract(
                "proxy",
                "pinned",
                "",
                &format!(r#"["implementation",{}]"#, id_ref("logic"))
            ),
            id_ref("proxy"),
            id_ref("logic"),
        ));
        let mut run = RunFile::empty("labcoat", Some("chain".into()));
        let mut next = 4;
        let v1 = artifacts(&[("logic", "v1"), ("proxy", "pp")]);
        settle(&graph, &v1, &mut run, &Options::default(), &mut next);
        // `settle` records only contracts and calls, so the setting has
        // no record yet: its value comes from the proxy's creation refs.
        run.steps.remove("proxy_implementation");
        assert_eq!(
            statuses(&graph, &v1, &run, &Options::default())[2].1,
            ActionStatus::Complete
        );

        // After the implementation is replaced, the proxy still points at
        // the instance it was created with.
        let v2 = artifacts(&[("logic", "v2"), ("proxy", "pp")]);
        settle(&graph, &v2, &mut run, &Options::default(), &mut next);
        run.steps.remove("proxy_implementation");
        let actions = reconcile(&graph, "labcoat", &v2, &facts(), &run, &Options::default())
            .unwrap()
            .actions;
        assert_eq!(actions[2].status, ActionStatus::Pending);
        assert_eq!(actions[2].detail.as_deref(), Some("2:5 → 2:7"));
    }

    #[test]
    fn calls_run_once_and_rerun_only_on_request() {
        let graph = graph(&format!(
            r#"{},{{"kind":"call","name":"seed","contract":{},"call":{{"method":"fund","args":{{}}}},"tx":{{}}}}"#,
            contract("token", "pinned", "", ""),
            id_ref("token"),
        ));
        let artifacts = artifacts(&[("token", "aa")]);
        let mut run = RunFile::empty("labcoat", Some("chain".into()));
        assert_eq!(
            statuses(&graph, &artifacts, &run, &Options::default())[1],
            ("seed".to_string(), ActionStatus::Pending, "call")
        );
        settle(&graph, &artifacts, &mut run, &Options::default(), &mut 4);
        assert_eq!(
            statuses(&graph, &artifacts, &run, &Options::default())[1].1,
            ActionStatus::Complete
        );
        assert_eq!(
            statuses(&graph, &artifacts, &run, &replace(&["seed"]))[1],
            ("seed".to_string(), ActionStatus::Pending, "rerun")
        );
    }

    #[test]
    fn replace_rejects_unknown_and_non_replaceable_names() {
        let graph = beacon_graph();
        let artifacts = artifacts(&[("logic", "v1"), ("beacon", "bb")]);
        let run = RunFile::empty("labcoat", None);
        for name in ["nope", "beacon_implementation"] {
            let error = reconcile(
                &graph,
                "labcoat",
                &artifacts,
                &facts(),
                &run,
                &replace(&[name]),
            )
            .err()
            .unwrap();
            assert_eq!(error.code, "CONFIG_INVALID");
        }
    }

    #[test]
    fn a_run_file_from_another_chain_is_only_ignored_on_regtest() {
        // Labcoat Network is reset routinely: stale records are skipped.
        let local = ToolkitConfig::default();
        assert!(refuse_foreign_chain(&local, "labcoat", true).is_ok());

        // A shared network is never reset, so a mismatch means the wrong
        // endpoint — refusing is what keeps apply from redeploying.
        let shared = ToolkitConfig {
            network: crate::NetworkTarget::Signet,
            ..ToolkitConfig::default()
        };
        assert!(refuse_foreign_chain(&shared, "signet", false).is_ok());
        let error = refuse_foreign_chain(&shared, "signet", true).unwrap_err();
        assert_eq!(error.code, "RUN_FILE_INVALID");
        assert!(error.message.contains("deployments/signet.json"));
    }

    #[test]
    fn decodes_setting_values_from_view_data() {
        let mut id = vec![0_u8; 32];
        id[0] = 2;
        id[16] = 7;
        assert_eq!(
            decode_setting_value(ValueType::AlkaneId, &format!("0x{}", hex::encode(&id))).unwrap(),
            "2:7"
        );
        assert_eq!(decode_setting_value(ValueType::U128, "0x2a").unwrap(), "42");
        assert!(decode_setting_value(ValueType::AlkaneId, "0x2a").is_err());
        assert_eq!(
            normalize_setting_value(ValueType::AlkaneId, "2:007").unwrap(),
            "2:7"
        );
    }
}
