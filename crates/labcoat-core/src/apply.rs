//! `labcoat plan` / `labcoat deploy` — reconcile the deployment graph a
//! project's `deploy.rs` declares against the network's run file and
//! chain state, then execute what has not run.
//!
//! A ledger, not convergence: Alkanes deployments are one-shot and
//! immutable, so a pinned step runs once and is then compared with its
//! record. An edited step is a `conflict` deploy refuses, never a diff it
//! repairs; `--replace <name>` deploys a new instance and keeps the old
//! record. Only two things change without that flag: a replaceable
//! implementation (new code deploys a new instance) and a setting (a
//! mutable value written when a read shows it differs).

use crate::abi::CallArgs;
use crate::deployment::{
    self, definition_hash, reference_identity, reference_key, Args, Contract, Field, Graph,
    Lifecycle, Resolved, Setting, Source, Step, TxShape, Value, ValueType,
};
use crate::error::{LabcoatError, Result};
use crate::execute::{self, Broadcast, ExecuteOutcome, TxOptions};
use crate::runfile::{self, RecordStatus, RunFile, StepRecord};
use crate::system::ToolkitConfig;
use crate::toolkit::{self, DeployTarget};
use crate::{abi, compile, lockfile, state, trace, workspace};
use alkanes_cli_common::provider::ConcreteProvider;
use alkanes_cli_common::traits::BitcoinRpcProvider;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Choices a human makes per invocation.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Pinned steps to run again: a contract deploys a new instance, a
    /// call executes again.
    pub replace: BTreeSet<String>,
    /// Run this step alone instead of every pending step. Everything it
    /// references must already be deployed, and it cannot be combined
    /// with `replace`, which is checked against the whole deployment.
    pub only: Option<String>,
}

// ---------------------------------------------------------------------------
// Plan model

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActionStatus {
    /// Runs on the next `deploy --broadcast`.
    Pending,
    /// Already ran with this definition; deploy skips it.
    Complete,
    /// Ran with a different definition, or references an instance that
    /// was since replaced. Deploy refuses until the step is restored or
    /// named in `--replace`.
    Conflict,
    /// A reserved id already holds matching code with no record; deploy
    /// records it without redeploying.
    Adopt,
    /// A previous deploy stopped mid-broadcast; verify that transaction
    /// before continuing.
    Interrupted,
    /// Cannot proceed (a reserve holds different code, a read failed, a
    /// step deployed on its own references one that has not run).
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
    /// The step's Alkane id, when known before it runs.
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
    /// The one step this plan covers, when a step was named.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub only: Option<String>,
    /// One action per declared step, or the named step's alone.
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

/// Everything deploy needs beyond the serializable plan.
pub struct PlanOutcome {
    pub plan: Plan,
    /// One action per declared step in declaration order, even when the
    /// plan itself covers a single step.
    actions: Vec<PlannedAction>,
    artifacts: BTreeMap<String, Artifact>,
    resolved: Resolved,
    run: RunFile,
    recovered: Vec<String>,
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
    /// is known before deploy.
    readings: BTreeMap<String, (String, Reading)>,
    ready_reveals: BTreeSet<String>,
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
        "DEPLOYMENT_BLOCKED",
        format!("{kind} `{name}` references a step that has no id yet"),
        "run the referenced step first — `labcoat plan` shows what is pending",
    )
}

/// Evaluate args to the form the ABI encoder takes, or None while any
/// reference is unresolved.
fn resolve_args(args: &Args, resolved: &Resolved, height: u64) -> Option<CallArgs> {
    resolve_args_with(args, |value| resolved.eval(value, height))
}

fn resolve_args_with(args: &Args, eval: impl Fn(&Value) -> Option<String>) -> Option<CallArgs> {
    if let Some(named) = &args.named {
        return named
            .iter()
            .map(|(name, value)| Some((name.clone(), eval(value)?)))
            .collect::<Option<Vec<_>>>()
            .map(CallArgs::Named);
    }
    args.positional
        .iter()
        .flatten()
        .map(eval)
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

fn definition_changed(step: &Step, wasm_sha256: Option<&str>, record: &StepRecord) -> bool {
    if record.definition.starts_with("v2:") {
        return record.definition != definition_hash(step, wasm_sha256);
    }
    if record.definition != deployment::legacy_definition_hash(step, wasm_sha256) {
        return true;
    }
    // Legacy definitions could hash different string arguments identically.
    // Compare with the concrete arguments at execution when they were recorded.
    let Some(height) = record.height else {
        return false;
    };
    let args = match step {
        Step::Contract(contract) if record.origin.as_deref() != Some("adopted") => {
            &contract.init.args
        }
        Step::Call(call) => &call.call.args,
        _ => return false,
    };
    let historical = resolve_args_with(args, |value| {
        value.eval_with(height, &|reference| {
            let id = record.refs.get(&reference_key(reference))?;
            match reference.field {
                Field::Id | Field::Created => Some(id.clone()),
                Field::Block => id.split_once(':').map(|(block, _)| block.to_string()),
                Field::Tx => id.split_once(':').map(|(_, tx)| tx.to_string()),
            }
        })
    });
    historical.is_some_and(|args| {
        let is_named = matches!(args, CallArgs::Named(_));
        let mut current = display_args(&args);
        if is_named {
            current.sort_unstable();
            let mut previous = record.args.clone();
            previous.sort_unstable();
            return current != previous;
        }
        current != record.args
    })
}

fn needs_confirmation(record: &StepRecord) -> bool {
    matches!(record.status, RecordStatus::Started)
        || (record.status == RecordStatus::Revert && !record.confirmed_revert)
}

fn interrupted_detail(name: &str, record: &StepRecord) -> String {
    if let Some(txid) = &record.txid {
        let replacement = if record.kind == "setting" {
            ""
        } else {
            " If it was dropped, pass `--replace` for this step only after verifying that."
        };
        return format!("transaction {txid} is awaiting an execution trace — run `labcoat deploy --broadcast` again after it is indexed.{replacement}");
    }
    if let Some(commit) = &record.commit_txid {
        return format!(
            "commit {commit} was prepared without a recorded reveal — check the wallet and import `{name}` if the deployment landed"
        );
    }
    format!(
        "an earlier deploy did not record a transaction id — check the wallet before retrying `{name}`"
    )
}

/// A write-only setting starts with the value last recorded for this
/// instance, or an explicitly identified argument from its initializer.
fn recorded_setting_value(setting: &Setting, target_id: &str, run: &RunFile) -> Option<String> {
    if let Some(record) = record_of(run, &setting.name, "setting") {
        if let Some(previous) = std::iter::once(record)
            .chain(record.history.iter().rev())
            .find(|entry| {
                entry.status == RecordStatus::Success && entry.id.as_deref() == Some(target_id)
            })
        {
            return previous.value.clone();
        }
    }
    let arg = setting.initializer_arg.as_deref()?;
    let Value::Ref(target) = &setting.contract else {
        return None;
    };
    let target_record = success_record(run, &target.step, "contract")?;
    if target_record.id.as_deref() != Some(target_id) {
        return None;
    }
    let value = target_record.args.iter().find_map(|entry| {
        entry
            .split_once('=')
            .and_then(|(name, value)| (name == arg).then_some(value))
    })?;
    normalize_setting_value(setting.value_type, value).ok()
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
    if let Some(only) = &options.only {
        match graph.steps.iter().find(|step| step.name() == only) {
            Some(Step::External(_)) => {
                return Err(LabcoatError::new(
                    "CONFIG_INVALID",
                    format!(
                        "`{only}` is an external reference: it names an Alkane that already exists, so there is nothing to deploy"
                    ),
                    "name a contract, call, or setting step from deploy.rs",
                ))
            }
            Some(_) => {}
            None => {
                return Err(LabcoatError::new(
                    "CONFIG_INVALID",
                    format!("deploy.rs declares no step named `{only}`"),
                    "declare the step in deploy.rs (`d.contract(name, package)` deploys a contract package), or run `labcoat plan` to list the declared steps",
                ))
            }
        }
        if !options.replace.is_empty() {
            return Err(LabcoatError::new(
                "CONFIG_INVALID",
                format!(
                    "--replace is checked against the whole deployment and cannot be combined with the step name `{only}`"
                ),
                "drop the step name: `labcoat deploy --replace <step> --broadcast` runs every pending step",
            ));
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
        if let Some(record) = run
            .steps
            .get(name)
            .filter(|record| record.kind != step.kind())
        {
            if !replace {
                action.status = ActionStatus::Conflict;
                action.detail = Some(format!(
                    "`{name}` was recorded as a {} and is now a {} — use a new name or explicitly replace a contract or call",
                    record.kind, step.kind(),
                ));
                actions.push(action);
                continue;
            }
        }

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
                    Some(record) if needs_confirmation(record) && !replace => {
                        if facts.ready_reveals.contains(name)
                            && !definition_changed(step, wasm_sha256, record)
                            && stale_refs(step, record, &resolved, &lifecycles).is_empty()
                        {
                            action.action = "resume";
                            action.detail = record.commit_txid.as_ref().map(|commit| {
                                format!("broadcasts the prepared reveal for commit {commit}")
                            });
                            action.args = record.args.clone();
                            pending_new(&mut action, &mut resolved)?;
                        } else {
                            action.status = ActionStatus::Interrupted;
                            action.detail = Some(interrupted_detail(name, record));
                            resolved.remove(name);
                        }
                    }
                    Some(record) if record.status == RecordStatus::Success => {
                        let changed = definition_changed(step, wasm_sha256, record);
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
                                        "already deployed on-chain with a matching ABI but not recorded — deploy records it without redeploying"
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
                action.args = preview_args(&call.call.args, &resolved, height);
                action.id = resolved.eval(&call.contract, height);
                match record_of(run, name, "call") {
                    Some(record) if needs_confirmation(record) && !replace => {
                        action.status = ActionStatus::Interrupted;
                        action.detail = Some(interrupted_detail(name, record));
                    }
                    Some(record) if record.status == RecordStatus::Success => {
                        let changed = definition_changed(step, None, record);
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
                if let Some(record) =
                    record_of(run, name, "setting").filter(|record| needs_confirmation(record))
                {
                    action.status = ActionStatus::Interrupted;
                    action.id = record.id.clone();
                    action.detail = Some(interrupted_detail(name, record));
                    actions.push(action);
                    continue;
                }
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
                            facts
                                .readings
                                .get(name)
                                .filter(|(read_id, _)| read_id == target_id)
                                .map(|(_, reading)| reading.clone())
                        } else {
                            recorded_setting_value(setting, target_id, run).map(Reading::Value)
                        };
                        match (current, desired) {
                            (Some(Reading::Failed(reason)), _) => {
                                action.status = ActionStatus::Blocked;
                                action.detail =
                                    Some(format!("cannot read the current value: {reason}"));
                            }
                            (Some(Reading::Value(current)), Some(desired))
                                if normalize_setting_value(setting.value_type, desired)
                                    .as_ref()
                                    .is_ok_and(|desired| &current == desired)
                                    && !step.uses_height() =>
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

    if let Some(only) = &options.only {
        block_on_unmet_references(graph, &mut actions, only);
    }

    Ok(Reconciled { actions, resolved })
}

/// A step that runs on its own needs everything it references to be
/// deployed already: the full run would have run those steps first.
fn block_on_unmet_references(graph: &Graph, actions: &mut [PlannedAction], only: &str) {
    let Some(index) = graph.steps.iter().position(|step| step.name() == only) else {
        return;
    };
    if !matches!(
        actions[index].status,
        ActionStatus::Pending | ActionStatus::Adopt
    ) {
        return;
    }
    let referenced: BTreeSet<&str> = graph.steps[index]
        .references()
        .into_iter()
        .map(|reference| reference.step.as_str())
        .collect();
    let unmet: Vec<String> = graph
        .steps
        .iter()
        .zip(actions.iter())
        .filter(|(step, action)| {
            referenced.contains(step.name()) && action.status != ActionStatus::Complete
        })
        .map(|(step, _)| format!("`{}`", step.name()))
        .collect();
    if unmet.is_empty() {
        return;
    }
    actions[index].status = ActionStatus::Blocked;
    actions[index].detail = Some(format!(
        "references {}, which {} not deployed yet — deploy {} first, or run `labcoat deploy` without a step name to run every pending step in order",
        unmet.join(", "),
        if unmet.len() == 1 { "is" } else { "are" },
        if unmet.len() == 1 { "it" } else { "them" },
    ));
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

async fn gather_facts(
    config: &ToolkitConfig,
    graph: &Graph,
    run: &RunFile,
    chain_id: Option<String>,
    height: u64,
) -> Result<Facts> {
    let known = known_ids(graph, config.network_id(), run);
    let mut facts = Facts {
        chain_id,
        height,
        ..Facts::default()
    };
    for (name, record) in &run.steps {
        if record.kind == "contract"
            && record.status == RecordStatus::Started
            && record.txid.is_none()
            && record
                .commit_txid
                .as_deref()
                .is_some_and(|commit| execute::load_pending_reveal(config, commit).is_some())
        {
            facts.ready_reveals.insert(name.clone());
        }
    }
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
                    facts
                        .readings
                        .insert(setting.name.clone(), (target, reading));
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

fn current_run(
    config: &ToolkitConfig,
    loaded: RunFile,
    observed: Option<&str>,
) -> Result<(RunFile, usize)> {
    let Some(recorded) = loaded.chain_id.as_deref() else {
        return Ok((loaded, 0));
    };
    let observed = observed.ok_or_else(|| {
        LabcoatError::new(
            "RUN_FILE_INVALID",
            format!(
                "cannot verify the chain for the `{}` deployment records",
                loaded.network
            ),
            "check the node and retry once its block 1 hash is available",
        )
    })?;
    let stale = recorded != observed;
    refuse_foreign_chain(config, &loaded.network, stale)?;
    if stale {
        let run = RunFile::empty(&loaded.network, Some(observed.to_string()));
        return Ok((run, loaded.steps.len()));
    }
    Ok((loaded, 0))
}

/// The kind of the deploy.rs step recorded as `name` on this chain, if
/// there is one. A deploy outside deploy.rs checks this so it never points
/// labcoat.lock at a contract the deployment's own record does not know.
pub async fn recorded_step_kind(
    config: &ToolkitConfig,
    root: &Path,
    name: &str,
) -> Result<Option<String>> {
    let loaded = runfile::load(root, config.network_id())?;
    if !loaded.steps.contains_key(name) {
        return Ok(None);
    }
    let chain_id = toolkit::chain_instance_id(config).await;
    let (run, _) = current_run(config, loaded, chain_id.as_deref())?;
    Ok(run.steps.get(name).map(|record| record.kind.clone()))
}

// An unknown trace, or a successful deploy trace without a creation ID,
// is still pending. Only an explicit execution outcome can settle it.
fn apply_trace(record: &mut StepRecord, status: &str, created: Vec<String>) -> bool {
    match status {
        "revert" => {
            record.status = RecordStatus::Revert;
            record.confirmed_revert = true;
        }
        "success" => {
            if record.kind == "contract" {
                let Some(id) = created.first() else {
                    return false;
                };
                record.id = Some(id.clone());
                record.created = created;
                if record.origin.is_none() {
                    record.origin = Some(
                        if record.wasm_sha256.is_some() {
                            "deployed"
                        } else {
                            "cloned"
                        }
                        .into(),
                    );
                }
            }
            record.status = RecordStatus::Success;
            record.confirmed_revert = false;
        }
        _ => return false,
    }
    record.updated_at = runfile::now_millis();
    true
}

async fn recover_broadcasts(provider: &ConcreteProvider, run: &mut RunFile) -> Vec<String> {
    let mut recovered = Vec::new();
    for (name, record) in &mut run.steps {
        if !needs_confirmation(record) {
            continue;
        }
        let Some(txid) = record.txid.as_deref() else {
            continue;
        };
        let traces = match trace::trace(provider, txid).await {
            Ok(traces) => traces,
            Err(error) => {
                tracing::debug!("execution trace for {txid} is unavailable: {error}");
                continue;
            }
        };
        let (status, _) = execute::find_return_status(&traces);
        let created = if record.kind == "contract" {
            execute::find_created_alkanes(&traces)
        } else {
            Vec::new()
        };
        if apply_trace(record, &status, created) {
            recovered.push(name.clone());
        }
    }
    recovered
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
    let provider = crate::system::connect(config, None, false).await?;
    let chain_id = BitcoinRpcProvider::get_block_hash(&provider, 1).await.ok();
    let height = BitcoinRpcProvider::get_block_count(&provider)
        .await
        .map_err(|error| LabcoatError::classify(error.into()))?;
    let (mut run, stale_records) = current_run(config, loaded, chain_id.as_deref())?;
    let recovered = recover_broadcasts(&provider, &mut run).await;
    let facts = gather_facts(config, graph, &run, chain_id, height).await?;

    let Reconciled { actions, resolved } =
        reconcile(graph, &network, &artifacts, &facts, &run, options)?;

    let declared: BTreeSet<&str> = graph.steps.iter().map(Step::name).collect();
    let undeclared = run
        .steps
        .keys()
        .filter(|name| !declared.contains(name.as_str()))
        .cloned()
        .collect();
    // A named step narrows what the plan reports and what deploy checks;
    // the full list stays aligned with the graph for execution.
    let shown: Vec<PlannedAction> = actions
        .iter()
        .filter(|action| {
            options
                .only
                .as_deref()
                .is_none_or(|only| action.name == only)
        })
        .cloned()
        .collect();
    let pending = shown
        .iter()
        .filter(|action| matches!(action.status, ActionStatus::Pending | ActionStatus::Adopt))
        .count();
    let conflicts = shown
        .iter()
        .filter(|action| action.status == ActionStatus::Conflict)
        .count();

    Ok(PlanOutcome {
        plan: Plan {
            script: deployment::SCRIPT.to_string(),
            network,
            chain_id: facts.chain_id,
            height: facts.height,
            only: options.only.clone(),
            actions: shown,
            stale_records,
            undeclared,
            pending,
            conflicts,
        },
        actions,
        artifacts,
        resolved,
        run,
        recovered,
    })
}

// ---------------------------------------------------------------------------
// Deploy

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedAction {
    pub kind: &'static str,
    pub name: String,
    /// executed | adopted | in-sync | skipped-complete
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
        format!("deploy stopped at {kind} `{name}`: {}", error.message),
        error.hint,
    )
}

fn reverted(what: &str, reason: Option<&str>) -> LabcoatError {
    LabcoatError::new(
        "EXECUTION_REVERT",
        format!("{what} reverted: {}", reason.unwrap_or("unknown reason")),
        "inspect the trace with `labcoat trace <txid>`",
    )
}

fn journal_broadcast<'a>(
    root: &'a Path,
    run: &'a mut RunFile,
    name: &'a str,
    record: &'a mut StepRecord,
) -> impl FnMut(Broadcast<'_>) -> Result<()> + 'a {
    |transaction| {
        match transaction {
            Broadcast::Commit(txid) => record.commit_txid = Some(txid.to_owned()),
            Broadcast::Execute(txid) => record.txid = Some(txid.to_owned()),
        }
        record.updated_at = runfile::now_millis();
        run.steps.insert(name.to_string(), record.clone());
        runfile::save(root, run)
    }
}

fn record_contract_address(
    config: &ToolkitConfig,
    root: &Path,
    run: &RunFile,
    name: &str,
    record: &StepRecord,
) -> Result<()> {
    let Some(id) = record.id.as_ref() else {
        return Err(unresolved("contract", name));
    };
    let origin = match record.origin.as_deref() {
        Some("adopted") => state::InstanceOrigin::Adopted,
        Some("imported") => state::InstanceOrigin::Imported,
        _ => state::InstanceOrigin::Deployed,
    };
    let deployment = lockfile::Deployment {
        alkanes_id: id.clone(),
        wasm_sha256: record.wasm_sha256.clone(),
        txid: record
            .txid
            .clone()
            .unwrap_or_else(|| record.origin.clone().unwrap_or_default()),
        block: None,
        status: "success".into(),
        deployed_at: record.updated_at,
        chain_id: run.chain_id.clone(),
    };
    let state_guard = state::deploy_guard(
        root,
        &config.environment,
        &state::observed_chain(config, run.chain_id.clone()),
    )?;
    lockfile::record(root, &run.network, name, deployment.clone())?;
    if let Some((mut lease, v2_state)) = state_guard {
        let same_instance = v2_state
            .resources
            .get(&state::resource_address(name))
            .and_then(|resource| resource.instances.last())
            .is_some_and(|instance| {
                instance.txid == deployment.txid
                    && instance.alkanes_id == deployment.alkanes_id
                    && instance.chain_id == deployment.chain_id
                    && instance.status == deployment.status
            });
        if !same_instance {
            state::record_instance(
                &mut lease,
                v2_state,
                name,
                &deployment,
                origin,
                state::InstanceExtras {
                    commit_txid: record.commit_txid.clone(),
                    labcoat_version: Some(env!("CARGO_PKG_VERSION").to_string()),
                    revert_reason: None,
                },
            )?;
        }
    }
    Ok(())
}

// The execution outcome is evidence only once the indexer sees a return or
// a revert. Pending attempts already have their IDs persisted by the observer.
fn finish_attempt(
    config: &ToolkitConfig,
    root: &Path,
    run: &mut RunFile,
    name: &str,
    mut record: StepRecord,
    outcome: &ExecuteOutcome,
) -> Result<()> {
    record.txid = Some(outcome.txid.clone());
    record.commit_txid = outcome.commit_txid.clone().or(record.commit_txid);
    let settled = apply_trace(&mut record, &outcome.status, outcome.created.clone());
    if record.kind == "contract" && record.status == RecordStatus::Success {
        record_contract_address(config, root, run, name, &record)?;
    }
    let kind = record.kind.clone();
    let status = record.status;
    run.steps.insert(name.to_string(), record);
    runfile::save(root, run)?;
    if !settled {
        return Err(LabcoatError::new(
            "DEPLOYMENT_BLOCKED",
            format!("waiting for an execution trace for {}", outcome.txid),
            "run `labcoat deploy --broadcast` again after the transaction is indexed",
        ));
    }
    if status == RecordStatus::Revert {
        return Err(reverted(&kind, outcome.revert_reason.as_deref()));
    }
    Ok(())
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

/// Execute the plan's pending steps in declaration order, or only the
/// step `options.only` names. Requires broadcast intent — the CLI only
/// calls this under `--broadcast`.
pub async fn apply(
    config: &ToolkitConfig,
    signer: &crate::signer::SignerSpec,
    root: &Path,
    graph: &Graph,
    options: &Options,
) -> Result<ApplyReport> {
    let _deployment_lock = runfile::lock(root)?;
    let PlanOutcome {
        plan,
        actions,
        artifacts,
        mut resolved,
        mut run,
        recovered,
    } = plan(config, root, graph, options).await?;

    run.chain_id = plan.chain_id.clone();
    if !recovered.is_empty() {
        for name in &recovered {
            if let Some(record) = success_record(&run, name, "contract") {
                record_contract_address(config, root, &run, name, record)?;
            }
        }
        runfile::save(root, &run)?;
    }

    // Fail fast on states that need a human, before any broadcast. With a
    // named step the plan holds that step alone, so only it is checked.
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
            if options.only.is_some() {
                "a step that ran is never changed in place — restore it in deploy.rs, or drop the step name and run `labcoat deploy --replace <step> --broadcast`; on Labcoat Network, `labcoat reset` starts a fresh chain"
            } else {
                "a step that ran is never changed in place — restore it in deploy.rs, or name it in `--replace` to run it again; on Labcoat Network, `labcoat reset` starts a fresh chain"
            },
        ));
    }
    for action in &plan.actions {
        if matches!(
            action.status,
            ActionStatus::Interrupted | ActionStatus::Blocked
        ) {
            return Err(LabcoatError::new(
                "DEPLOYMENT_BLOCKED",
                format!(
                    "{} `{}` cannot proceed: {}",
                    action.kind,
                    action.name,
                    action.detail.as_deref().unwrap_or("unknown reason")
                ),
                "resolve the reported state and run `labcoat deploy --broadcast` again",
            ));
        }
    }

    let network = plan.network.clone();
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

    for (step, action) in graph.steps.iter().zip(&actions) {
        let name = step.name();
        // A named step runs alone. External references broadcast nothing,
        // so they are still recorded.
        if options.only.as_deref().is_some_and(|only| only != name)
            && !matches!(step, Step::External(_))
        {
            continue;
        }
        // A preview uses the planning tip. Execution resolves height at the
        // step that consumes it, after earlier transactions may have mined.
        let height = if action.status == ActionStatus::Pending && step.uses_height() {
            toolkit::chain_height(config)
                .await
                .map_err(|error| stop(step.kind(), name, error))?
        } else {
            plan.height
        };
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
                let previous = run.steps.get(name).cloned();
                let mut record = StepRecord::new("contract", RecordStatus::Started, definition)
                    .replacing(previous.as_ref());
                record.wasm_sha256 = wasm_sha256.clone();
                record.artifact = artifact.and_then(Artifact::stem);
                record.height = Some(height);
                record.refs = current_refs(step, &resolved);

                if action.status == ActionStatus::Adopt {
                    let id = action.id.clone().expect("adopted reserves have an id");
                    record.status = RecordStatus::Success;
                    record.id = Some(id.clone());
                    record.created = vec![id.clone()];
                    record.origin = Some("adopted".into());
                    record_contract_address(config, root, &run, name, &record)?;
                    run.steps.insert(name.to_string(), record);
                    runfile::save(root, &run)?;
                    resolved.insert(name, &id, std::slice::from_ref(&id))?;
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
                let outcome = if action.action == "resume" {
                    record = previous.expect("resume has a recorded commit");
                    let reveal = record
                        .commit_txid
                        .as_deref()
                        .and_then(|id| execute::load_pending_reveal(config, id))
                        .ok_or_else(|| {
                            fail(LabcoatError::new(
                                "DEPLOYMENT_BLOCKED",
                                "the prepared reveal is unavailable",
                                "restore the pending reveal file or inspect the recorded commit",
                            ))
                        })?;
                    toolkit::resume_deploy(
                        config,
                        signer,
                        root,
                        reveal,
                        &mut journal_broadcast(root, &mut run, name, &mut record),
                    )
                    .await
                    .map_err(fail)?
                } else {
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
                            let method =
                                abi::encode_invocation(abi_bytes.as_deref(), method, &args)
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
                    record.origin = Some(if is_clone { "cloned" } else { "deployed" }.into());
                    toolkit::deploy_with_broadcast(
                        config,
                        signer,
                        root,
                        toolkit::DeployRequest {
                            wasm_path: artifact
                                .map(|artifact| artifact.wasm_path.as_path())
                                .unwrap_or(Path::new("")),
                            contract_name: None,
                            initializer,
                            cellpack_args: &cellpack_args,
                            fee_rate: config.fee_rate,
                            target,
                            options: &tx_options,
                        },
                        Some(&mut journal_broadcast(root, &mut run, name, &mut record)),
                    )
                    .await
                    .map_err(fail)?
                };
                finish_attempt(config, root, &mut run, name, record, &outcome).map_err(fail)?;
                let recorded = &run.steps[name];
                let id = recorded
                    .id
                    .as_deref()
                    .expect("confirmed contract has an id");
                resolved.insert(name, id, &recorded.created).map_err(fail)?;
                report.actions.push(AppliedAction {
                    kind: "contract",
                    name: name.to_string(),
                    outcome: "executed".into(),
                    id: Some(id.to_string()),
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

                let mut record =
                    StepRecord::new("call", RecordStatus::Started, definition_hash(step, None))
                        .replacing(run.steps.get(name));
                record.id = Some(target_id.clone());
                record.refs = current_refs(step, &resolved);
                record.args = display_args(&args);
                record.height = Some(height);

                let outcome = toolkit::call_with_broadcast(
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
                    Some(&mut journal_broadcast(root, &mut run, name, &mut record)),
                )
                .await
                .map_err(fail)?;
                finish_attempt(config, root, &mut run, name, record, &outcome).map_err(fail)?;
                report.actions.push(AppliedAction {
                    kind: "call",
                    name: name.to_string(),
                    outcome: "executed".into(),
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
                                "DEPLOYMENT_BLOCKED",
                                format!("cannot read the current value: {reason}"),
                                "check the setting's read method",
                            )))
                        }
                    }
                } else {
                    recorded_setting_value(setting, &target_id, &run)
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

                let mut started = record.replacing(previous.as_ref());
                started.status = RecordStatus::Started;
                let outcome = toolkit::call_with_broadcast(
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
                    Some(&mut journal_broadcast(root, &mut run, name, &mut started)),
                )
                .await
                .map_err(fail)?;
                finish_attempt(config, root, &mut run, name, started, &outcome).map_err(fail)?;
                // Read the value back: a write that succeeds without
                // taking effect must not be recorded as executed.
                if setting.read.is_some() {
                    let after = read_setting(config, setting, &target_id)
                        .await
                        .map_err(fail)?;
                    if after != Reading::Value(desired.clone()) {
                        return Err(fail(LabcoatError::new(
                            "DEPLOYMENT_BLOCKED",
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
                report.actions.push(AppliedAction {
                    kind: "setting",
                    name: name.to_string(),
                    outcome: "executed".into(),
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

/// Record a contract that already exists on chain as a step's instance.
pub async fn import(
    config: &ToolkitConfig,
    root: &Path,
    graph: &Graph,
    name: &str,
    id: &str,
    deployment_txid: Option<&str>,
) -> Result<ImportReport> {
    let _deployment_lock = runfile::lock(root)?;
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
    let (mut run, _) = current_run(config, loaded, chain_id.as_deref())?;
    run.chain_id = chain_id;
    let previous = run.steps.get(name);
    if let Some(existing) = previous {
        if existing.kind != "contract"
            || (existing.status == RecordStatus::Success && existing.id.as_deref() != Some(&id))
        {
            return Err(LabcoatError::new(
                "DEPLOYMENT_CONFLICT",
                format!(
                    "`{name}` already has a {} record for another step or instance",
                    existing.kind
                ),
                "use a different step name, or replace the recorded step with `labcoat deploy --replace <name>`",
            ));
        }
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

    let txid = match deployment_txid {
        Some(txid) => Some(txid.parse::<bitcoin::Txid>().map_err(|error| {
            LabcoatError::new(
                "CONFIG_INVALID",
                format!("invalid deployment transaction id: {error}"),
                "pass the Bitcoin transaction id of the deploy or reveal",
            )
        })?),
        None => {
            let ledger = lockfile::load(root)?;
            previous
                .and_then(|record| record.txid.as_deref())
                .and_then(|txid| txid.parse().ok())
                .or_else(|| {
                    ledger.networks.get(&network)?.values().find_map(|record| {
                        (record.alkanes_id == id && record.status == "success")
                            .then(|| record.txid.parse().ok())
                            .flatten()
                    })
                })
        }
    };
    let existing = success_record(&run, name, "contract");
    let required_index = graph
        .steps
        .iter()
        .flat_map(Step::references)
        .filter(|reference| reference.step == name && reference.field == Field::Created)
        .filter_map(|reference| reference.index)
        .max();

    let created = if let Some(txid) = txid {
        let traces = toolkit::trace(config, &txid.to_string(), false).await?;
        imported_creations(&id, traces).ok_or_else(|| {
            LabcoatError::new(
                "DEPLOYMENT_BLOCKED",
                format!("transaction {txid} has no successful deployment trace for {id}"),
                "check the reveal transaction id and wait for it to be indexed",
            )
        })?
    } else {
        existing
            .filter(|record| !record.created.is_empty())
            .map(|record| record.created.clone())
            .unwrap_or_else(|| vec![id.clone()])
    };
    if required_index.is_some_and(|index| index >= created.len()) {
        return Err(LabcoatError::new(
            "DEPLOYMENT_BLOCKED",
            format!(
                "`{name}` needs additional Alkane IDs from its deployment, such as its auth token"
            ),
            "pass `labcoat import <name> <id> --txid <reveal-txid>` to recover the created IDs",
        ));
    }

    let resolved = known_ids(graph, &network, &run);
    let mut record = existing.cloned().unwrap_or_else(|| {
        let wasm_sha256 = artifact.map(|artifact| artifact.wasm_sha256.clone());
        let mut record = StepRecord::new(
            "contract",
            RecordStatus::Success,
            definition_hash(step, wasm_sha256.as_deref()),
        )
        .replacing(previous);
        record.id = Some(id.clone());
        record.refs = current_refs(step, &resolved);
        record.wasm_sha256 = wasm_sha256;
        record.artifact = artifact.and_then(Artifact::stem);
        record.commit_txid = previous.and_then(|prev| prev.commit_txid.clone());
        record.origin = Some("imported".into());
        record
    });
    record.created = created;
    if let Some(txid) = txid {
        record.txid = Some(txid.to_string());
    }
    record_contract_address(config, root, &run, name, &record)?;
    run.steps.insert(name.to_string(), record);
    runfile::save(root, &run)?;

    Ok(ImportReport {
        network,
        name: name.to_string(),
        id,
        abi_verified,
    })
}

fn imported_creations(id: &str, traces: Option<Vec<serde_json::Value>>) -> Option<Vec<String>> {
    traces?.into_iter().find_map(|trace| {
        let trace = Some(vec![trace]);
        let (status, _) = execute::find_return_status(&trace);
        if status != "success" {
            return None;
        }
        let created = execute::find_created_alkanes(&trace);
        (created.first().is_some_and(|created_id| created_id == id)).then_some(created)
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

    /// Record every pending contract and call as if deploy had run it,
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
            let args = match step {
                Step::Contract(contract) => &contract.init.args,
                Step::Call(call) => &call.call.args,
                _ => continue,
            };
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
            record.height = Some(facts().height);
            record.args = display_args(&resolve_args(args, &resolved, facts().height).unwrap());
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
            ..Options::default()
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
    fn a_fresh_graph_is_pending_and_a_deployed_one_is_complete() {
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
    fn reserves_resolve_before_deploy_and_occupied_ones_adopt_or_block() {
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
            facts.readings.insert(
                "beacon_implementation".into(),
                ("2:6".into(), Reading::Value(value.into())),
            );
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
        // known until deploy.
        let v2 = artifacts(&[("logic", "v2"), ("beacon", "bb")]);
        let (upgrade, detail) = status(&read("2:5"), &v2, &run);
        assert_eq!(upgrade, ActionStatus::Pending);
        assert_eq!(detail.as_deref(), Some("2:5 → <logic.id>"));
        // An unreadable value blocks rather than guessing.
        let mut failed = facts();
        failed.readings.insert(
            "beacon_implementation".into(),
            ("2:6".into(), Reading::Failed("boom".into())),
        );
        assert_eq!(status(&failed, &v1, &run).0, ActionStatus::Blocked);
    }

    #[test]
    fn a_read_less_setting_starts_from_what_the_initializer_was_given() {
        let graph = graph(&format!(
            r#"{},{},{{"kind":"setting","name":"proxy_implementation","contract":{},"value":{},"valueType":"alkane-id","write":"upgrade","initializerArg":"implementation","tx":{{}}}}"#,
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
    fn first_write_only_setting_is_pending() {
        let market = contract("market", "pinned", "", "");
        let deployed = graph(&market);
        let graph = graph(&format!(
            r#"{market},{{"kind":"setting","name":"fee","contract":{},"value":{{"lit":"30"}},"valueType":"u128","write":"set_fee","tx":{{}}}}"#,
            id_ref("market"),
        ));
        let artifacts = artifacts(&[("market", "v1")]);
        let mut run = RunFile::empty("labcoat", None);
        settle(&deployed, &artifacts, &mut run, &Options::default(), &mut 4);
        assert_eq!(
            statuses(&graph, &artifacts, &run, &Options::default())[1].1,
            ActionStatus::Pending
        );
    }

    #[test]
    fn step_kind_changes_require_replacement_and_keep_history() {
        let token = contract("token", "pinned", "", "");
        let original = graph(&format!(
            r#"{token},{{"kind":"call","name":"seed","contract":{},"call":{{"method":"fund","args":{{}}}},"tx":{{}}}}"#,
            id_ref("token"),
        ));
        let artifacts = artifacts(&[("token", "v1"), ("seed", "v1")]);
        let mut run = RunFile::empty("labcoat", None);
        let mut next = 4;
        settle(
            &original,
            &artifacts,
            &mut run,
            &Options::default(),
            &mut next,
        );
        let edited = graph(&format!("{token},{}", contract("seed", "pinned", "", "")));
        assert_eq!(
            statuses(&edited, &artifacts, &run, &Options::default())[1].1,
            ActionStatus::Conflict
        );
        settle(
            &edited,
            &artifacts,
            &mut run,
            &replace(&["seed"]),
            &mut next,
        );
        assert_eq!(run.steps["seed"].kind, "contract");
        assert_eq!(run.steps["seed"].history[0].kind, "call");
    }

    #[test]
    fn definitions_distinguish_argument_boundaries_and_accept_matching_legacy_records() {
        let version = |first: &str, second: &str| {
            graph(&format!(
                r#"{{"kind":"external","name":"registry","ids":{{"*":[2,9]}}}},{{"kind":"call","name":"configure","contract":{},"call":{{"method":"set_labels","args":{{"positional":[{{"lit":"{first}"}},{{"lit":"{second}"}}]}}}},"tx":{{}}}}"#,
                id_ref("registry"),
            ))
        };
        let first = version("a,b", "c");
        let second = version("a", "b,c");
        assert_ne!(
            definition_hash(&first.steps[1], None),
            definition_hash(&second.steps[1], None)
        );
        let mut run = RunFile::empty("labcoat", None);
        let artifacts = BTreeMap::new();
        settle(&first, &artifacts, &mut run, &Options::default(), &mut 4);
        run.steps.get_mut("configure").unwrap().definition =
            deployment::legacy_definition_hash(&first.steps[1], None);
        assert_eq!(
            statuses(&first, &artifacts, &run, &Options::default())[1].1,
            ActionStatus::Complete
        );
        assert_eq!(
            statuses(&second, &artifacts, &run, &Options::default())[1].1,
            ActionStatus::Conflict
        );
    }

    #[test]
    fn unknown_broadcasts_stay_interrupted_until_the_trace_is_definitive() {
        let graph = token_and_market();
        let artifacts = artifacts(&[("token", "aa"), ("market", "bb")]);
        let mut run = RunFile::empty("labcoat", None);
        let mut record = StepRecord::new(
            "contract",
            RecordStatus::Started,
            definition_hash(&graph.steps[0], Some("aa")),
        );
        record.txid = Some("f0".repeat(32));
        assert!(!apply_trace(&mut record, "unknown", Vec::new()));
        assert!(!apply_trace(&mut record, "success", Vec::new()));
        run.steps.insert("token".into(), record.clone());
        assert_eq!(
            statuses(&graph, &artifacts, &run, &Options::default())[0].1,
            ActionStatus::Interrupted
        );

        // An older unknown result was saved as a revert and still needs a trace.
        record.status = RecordStatus::Revert;
        run.steps.insert("token".into(), record.clone());
        assert_eq!(
            statuses(&graph, &artifacts, &run, &Options::default())[0].1,
            ActionStatus::Interrupted
        );
        let mut reverted = record.clone();
        assert!(apply_trace(&mut reverted, "revert", Vec::new()));
        run.steps.insert("token".into(), reverted);
        assert_eq!(
            statuses(&graph, &artifacts, &run, &Options::default())[0].1,
            ActionStatus::Pending
        );

        assert!(apply_trace(
            &mut record,
            "success",
            vec!["2:5".into(), "2:6".into()]
        ));
        assert_eq!(record.created, ["2:5", "2:6"]);
        run.steps.insert("token".into(), record);
        assert_eq!(
            statuses(&graph, &artifacts, &run, &Options::default())[0].1,
            ActionStatus::Complete
        );
    }

    #[test]
    fn import_uses_the_successful_trace_for_the_requested_contract() {
        let traces = Some(vec![
            serde_json::json!({"trace": [
                {"type": "create_alkane", "alkane_id": {"block": 4, "tx": 1}},
                {"type": "return"}
            ]}),
            serde_json::json!({"trace": [
                {"type": "create_alkane", "alkane_id": {"block": 2, "tx": 7}},
                {"type": "call"},
                {"type": "create_alkane", "alkane_id": {"block": 2, "tx": 8}},
                {"type": "call"},
                {"type": "return"},
                {"type": "return"}
            ]}),
        ]);
        assert_eq!(
            imported_creations("2:7", traces.clone()).unwrap(),
            ["2:7", "2:8"]
        );
        assert!(imported_creations("2:8", traces).is_none());
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

    fn only(name: &str) -> Options {
        Options {
            only: Some(name.to_string()),
            ..Options::default()
        }
    }

    #[test]
    fn a_named_step_waits_for_what_it_references() {
        let graph = token_and_market();
        let artifacts = artifacts(&[("token", "aa"), ("market", "bb")]);
        let mut run = RunFile::empty("labcoat", Some("chain".into()));

        // `token` references nothing, so it can run on its own.
        assert_eq!(
            statuses(&graph, &artifacts, &run, &only("token"))[0],
            ("token".to_string(), ActionStatus::Pending, "create")
        );
        // `market` references `token`, which has not run.
        let planned = reconcile(
            &graph,
            "labcoat",
            &artifacts,
            &facts(),
            &run,
            &only("market"),
        )
        .unwrap()
        .actions;
        assert_eq!(planned[1].status, ActionStatus::Blocked);
        assert!(planned[1].detail.as_deref().unwrap().contains("`token`"));
        // Naming a step never changes how the other steps reconcile.
        assert_eq!(planned[0].status, ActionStatus::Pending);

        // Once `token` is recorded, `market` is free to run.
        let token_alone = graph_of_token();
        settle(
            &token_alone,
            &artifacts,
            &mut run,
            &Options::default(),
            &mut 4,
        );
        assert_eq!(
            statuses(&graph, &artifacts, &run, &only("market")),
            [
                ("token".to_string(), ActionStatus::Complete, "none"),
                ("market".to_string(), ActionStatus::Pending, "create"),
            ]
        );
    }

    fn graph_of_token() -> Graph {
        graph(&contract(
            "token",
            "pinned",
            "",
            r#"["supply",{"lit":"100"}]"#,
        ))
    }

    #[test]
    fn a_named_step_that_already_ran_keeps_its_status() {
        let graph = token_and_market();
        let artifacts = artifacts(&[("token", "aa"), ("market", "bb")]);
        let mut run = RunFile::empty("labcoat", Some("chain".into()));
        settle(&graph, &artifacts, &mut run, &Options::default(), &mut 4);

        // Unchanged: nothing to do.
        assert_eq!(
            statuses(&graph, &artifacts, &run, &only("market"))[1].1,
            ActionStatus::Complete
        );
        // Edited: still a conflict, exactly as in the full plan.
        let rebuilt = self::artifacts(&[("token", "aa"), ("market", "cc")]);
        assert_eq!(
            statuses(&graph, &rebuilt, &run, &only("market"))[1].1,
            ActionStatus::Conflict
        );
    }

    #[test]
    fn a_named_step_must_be_declared_deployable_and_not_replaced() {
        let graph = graph(&format!(
            r#"{{"kind":"external","name":"usd","ids":{{"*":[4,1]}}}},{}"#,
            contract(
                "market",
                "pinned",
                "",
                &format!(r#"["quote",{}]"#, id_ref("usd"))
            )
        ));
        let artifacts = artifacts(&[("market", "bb")]);
        let run = RunFile::empty("labcoat", Some("chain".into()));
        let refused = |options: Options| {
            reconcile(&graph, "labcoat", &artifacts, &facts(), &run, &options)
                .err()
                .unwrap()
        };

        let undeclared = refused(only("nope"));
        assert_eq!(undeclared.code, "CONFIG_INVALID");
        assert!(undeclared.message.contains("no step named `nope`"));

        let external = refused(only("usd"));
        assert_eq!(external.code, "CONFIG_INVALID");
        assert!(external.message.contains("external reference"));

        let replaced = refused(Options {
            replace: ["market".to_string()].into(),
            only: Some("market".to_string()),
        });
        assert_eq!(replaced.code, "CONFIG_INVALID");
        assert!(replaced.message.contains("--replace"));

        // An external is always resolved, so a step that references only
        // externals can run on its own.
        assert_eq!(
            statuses(&graph, &artifacts, &run, &only("market"))[1],
            ("market".to_string(), ActionStatus::Pending, "create")
        );
    }

    #[test]
    fn a_run_file_from_another_chain_is_only_ignored_on_regtest() {
        // Labcoat Network is reset routinely: stale records are skipped.
        let local = ToolkitConfig::default();
        assert!(refuse_foreign_chain(&local, "labcoat", true).is_ok());

        // A shared network is never reset, so a mismatch means the wrong
        // endpoint — refusing is what keeps deploy from redeploying.
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
