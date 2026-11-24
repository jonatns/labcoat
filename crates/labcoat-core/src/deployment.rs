//! The deployment graph — what a project's `deploy.rs` script declares and
//! `labcoat plan` / `labcoat apply` reconcile.
//!
//! The script (see `labcoat_deploy`) never executes anything: it
//! emits this graph as JSON and the CLI validates, plans, and applies it.
//! Steps run in declaration order, and a step may reference only steps
//! declared before it, so the declaration order is always a valid
//! dependency order.
//!
//! Step lifecycles:
//!
//! - `external`: a name for an id that exists outside the deployment.
//! - `contract` (pinned): deployed once. Its *definition* — artifact hash,
//!   target, initializer, and symbolic arguments — is recorded when it
//!   runs; a later change is a conflict, not something apply repairs.
//! - `contract` (replaceable): code reached through an upgradeable
//!   pointer. A changed definition deploys a new instance.
//! - `call` (pinned): runs once.
//! - `setting`: a mutable on-chain value apply keeps equal to the declared
//!   one, writing only when a read shows a difference.

use crate::error::{LabcoatError, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// The deployment script, relative to the project root.
pub const SCRIPT: &str = "deploy.rs";
/// The Cargo example target that runs the script.
pub const SCRIPT_TARGET: &str = "deploy";
pub const GRAPH_SCHEMA: &str = "labcoat/deployment/v1";

fn invalid(message: impl Into<String>) -> LabcoatError {
    LabcoatError::new(
        "DEPLOY_SCRIPT_INVALID",
        message.into(),
        "fix deploy.rs — see `labcoat docs --llm` for the deployment script API",
    )
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Graph {
    pub schema: String,
    pub network: String,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Step {
    External(External),
    Contract(Contract),
    Call(Call),
    Setting(Setting),
}

impl Step {
    pub fn name(&self) -> &str {
        match self {
            Step::External(step) => &step.name,
            Step::Contract(step) => &step.name,
            Step::Call(step) => &step.name,
            Step::Setting(step) => &step.name,
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Step::External(_) => "external",
            Step::Contract(_) => "contract",
            Step::Call(_) => "call",
            Step::Setting(_) => "setting",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct External {
    pub name: String,
    /// Network name (or `*` for every network) → `[block, tx]`.
    pub ids: BTreeMap<String, (u64, u64)>,
}

impl External {
    pub fn id_for(&self, network: &str) -> Option<(u64, u64)> {
        self.ids.get(network).or_else(|| self.ids.get("*")).copied()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Lifecycle {
    Pinned,
    Replaceable,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Source {
    /// A Cargo contract package in the project workspace.
    Package { package: String },
    /// A prebuilt raw `.wasm`, relative to the project root.
    Wasm { wasm: String },
    /// The code of an already-deployed template.
    Clone { of: Value },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Contract {
    pub name: String,
    pub source: Source,
    pub lifecycle: Lifecycle,
    #[serde(default)]
    pub init: Invocation,
    #[serde(default)]
    pub tx: TxShape,
    /// Reserved number N (decimal), deploying to `4:N`.
    #[serde(default)]
    pub reserve: Option<String>,
}

impl Contract {
    pub fn reserve_number(&self) -> Result<Option<u128>> {
        match &self.reserve {
            None => Ok(None),
            Some(text) => text.parse().map(Some).map_err(|_| {
                invalid(format!(
                    "contract `{}`: reserve `{text}` is not a number",
                    self.name
                ))
            }),
        }
    }
}

/// A method plus arguments: a contract's initializer or a call.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Invocation {
    /// ABI method name or decimal opcode. Absent on a contract: the ABI's
    /// opcode-0 constructor.
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub args: Args,
    /// Step whose ABI resolves `method` (calls through a proxy).
    #[serde(default)]
    pub abi: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Args {
    #[serde(default)]
    pub named: Option<Vec<(String, Value)>>,
    #[serde(default)]
    pub positional: Option<Vec<Value>>,
}

impl Args {
    pub fn is_empty(&self) -> bool {
        self.named.as_ref().is_none_or(Vec::is_empty)
            && self.positional.as_ref().is_none_or(Vec::is_empty)
    }

    pub fn values(&self) -> impl Iterator<Item = &Value> {
        self.named
            .iter()
            .flatten()
            .map(|(_, value)| value)
            .chain(self.positional.iter().flatten())
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TxShape {
    #[serde(default)]
    pub inputs: Vec<Value>,
    #[serde(default)]
    pub to: Option<Value>,
    #[serde(default)]
    pub pointer: Option<String>,
    #[serde(default)]
    pub refund: Option<String>,
    #[serde(default)]
    pub edicts: Vec<Value>,
}

impl TxShape {
    pub fn values(&self) -> impl Iterator<Item = &Value> {
        self.inputs
            .iter()
            .chain(self.to.iter())
            .chain(self.edicts.iter())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Call {
    pub name: String,
    pub contract: Value,
    pub call: Invocation,
    #[serde(default)]
    pub tx: TxShape,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum ValueType {
    #[serde(rename = "alkane-id")]
    AlkaneId,
    #[serde(rename = "u128")]
    U128,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Setting {
    pub name: String,
    pub contract: Value,
    pub value: Value,
    pub value_type: ValueType,
    /// View returning the current value; absent means the recorded value
    /// is the only source of truth.
    #[serde(default)]
    pub read: Option<String>,
    /// Named initializer argument that sets this value on a newly deployed target.
    #[serde(default)]
    pub initializer_arg: Option<String>,
    /// Method that sets the value.
    #[serde(default)]
    pub write: Option<String>,
    #[serde(default)]
    pub tx: TxShape,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Field {
    Id,
    Block,
    Tx,
    /// The `index`-th Alkane created by a contract step's transaction.
    Created,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    pub step: String,
    pub field: Field,
    #[serde(default)]
    pub index: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HeightOffset {
    pub offset: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Value {
    Lit(String),
    Ref(Reference),
    /// The chain height when the step runs, plus an offset.
    Height(HeightOffset),
    Concat(Vec<Value>),
}

impl Value {
    /// Every reference inside this value.
    pub fn references<'a>(&'a self, out: &mut Vec<&'a Reference>) {
        match self {
            Value::Lit(_) | Value::Height(_) => {}
            Value::Ref(reference) => out.push(reference),
            Value::Concat(parts) => parts.iter().for_each(|part| part.references(out)),
        }
    }

    pub(crate) fn uses_height(&self) -> bool {
        match self {
            Self::Height(_) => true,
            Self::Concat(parts) => parts.iter().any(Self::uses_height),
            _ => false,
        }
    }

    pub(crate) fn eval_with(
        &self,
        height: u64,
        resolve: &impl Fn(&Reference) -> Option<String>,
    ) -> Option<String> {
        match self {
            Self::Lit(text) => Some(text.clone()),
            Self::Ref(reference) => resolve(reference),
            Self::Height(HeightOffset { offset }) => {
                Some((height as i128 + *offset as i128).max(0).to_string())
            }
            Self::Concat(parts) => parts
                .iter()
                .map(|part| part.eval_with(height, resolve))
                .collect::<Option<Vec<_>>>()
                .map(|parts| parts.concat()),
        }
    }

    /// The symbolic source form for plan output.
    pub fn render(&self) -> String {
        match self {
            Value::Lit(text) => text.clone(),
            Value::Ref(reference) => match (reference.field, reference.index) {
                (Field::Id, _) => format!("<{}.id>", reference.step),
                (Field::Block, _) => format!("<{}.block>", reference.step),
                (Field::Tx, _) => format!("<{}.tx>", reference.step),
                (Field::Created, index) => {
                    format!("<{}.created[{}]>", reference.step, index.unwrap_or(0))
                }
            },
            Value::Height(height) => match height.offset {
                0 => "<height>".to_string(),
                offset if offset > 0 => format!("<height+{offset}>"),
                offset => format!("<height{offset}>"),
            },
            Value::Concat(parts) => parts.iter().map(Value::render).collect(),
        }
    }
}

/// Ids known for a step: the contract or external itself, plus every
/// Alkane a contract's transaction created (index 0 is the contract).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepIds {
    pub id: (u128, u128),
    pub created: Vec<String>,
}

/// Ids resolved so far, by step name.
#[derive(Debug, Clone, Default)]
pub struct Resolved {
    ids: BTreeMap<String, StepIds>,
}

pub fn parse_id(id: &str) -> Result<(u128, u128)> {
    let bad = || invalid(format!("`{id}` is not a block:tx Alkane id"));
    let (block, tx) = id.split_once(':').ok_or_else(bad)?;
    Ok((
        block.trim().parse().map_err(|_| bad())?,
        tx.trim().parse().map_err(|_| bad())?,
    ))
}

impl Resolved {
    pub fn insert(&mut self, name: &str, id: &str, created: &[String]) -> Result<()> {
        self.ids.insert(
            name.to_string(),
            StepIds {
                id: parse_id(id)?,
                created: created.to_vec(),
            },
        );
        Ok(())
    }

    pub fn remove(&mut self, name: &str) {
        self.ids.remove(name);
    }

    pub fn id(&self, name: &str) -> Option<String> {
        self.ids
            .get(name)
            .map(|ids| format!("{}:{}", ids.id.0, ids.id.1))
    }

    /// One reference's value, or None while its step has no id yet.
    pub fn reference(&self, reference: &Reference) -> Option<String> {
        let ids = self.ids.get(&reference.step)?;
        match reference.field {
            Field::Id => Some(format!("{}:{}", ids.id.0, ids.id.1)),
            Field::Block => Some(ids.id.0.to_string()),
            Field::Tx => Some(ids.id.1.to_string()),
            Field::Created => ids.created.get(reference.index.unwrap_or(0)).cloned(),
        }
    }

    /// Evaluate to a string, or None while any reference is unresolved.
    pub fn eval(&self, value: &Value, height: u64) -> Option<String> {
        value.eval_with(height, &|reference| self.reference(reference))
    }

    /// Evaluate for display: resolved where possible, symbolic otherwise.
    pub fn preview(&self, value: &Value, height: u64) -> String {
        self.eval(value, height).unwrap_or_else(|| value.render())
    }
}

/// The key a step's consumed reference is recorded under.
pub fn reference_key(reference: &Reference) -> String {
    match reference.field {
        Field::Created => format!("{}#{}", reference.step, reference.index.unwrap_or(0)),
        _ => reference.step.clone(),
    }
}

/// The value recorded for a consumed reference: always a full id, so a
/// `.block` or `.tx` reference still detects a replaced instance.
pub fn reference_identity(resolved: &Resolved, reference: &Reference) -> Option<String> {
    match reference.field {
        Field::Created => resolved.reference(reference),
        _ => resolved.id(&reference.step),
    }
}

impl Step {
    /// Every value the step evaluates when it runs.
    pub fn values(&self) -> Vec<&Value> {
        match self {
            Step::External(_) => Vec::new(),
            Step::Contract(step) => {
                let mut values: Vec<&Value> = step.init.args.values().collect();
                values.extend(step.tx.values());
                if let Source::Clone { of } = &step.source {
                    values.push(of);
                }
                values
            }
            Step::Call(step) => {
                let mut values = vec![&step.contract];
                values.extend(step.call.args.values());
                values.extend(step.tx.values());
                values
            }
            Step::Setting(step) => {
                let mut values = vec![&step.contract, &step.value];
                values.extend(step.tx.values());
                values
            }
        }
    }

    pub(crate) fn uses_height(&self) -> bool {
        self.values().into_iter().any(Value::uses_height)
    }

    pub fn references(&self) -> Vec<&Reference> {
        let mut references = Vec::new();
        for value in self.values() {
            value.references(&mut references);
        }
        references
    }
}

/// Hash the symbolic definition with its types and argument boundaries intact.
pub fn definition_hash(step: &Step, wasm_sha256: Option<&str>) -> String {
    use serde_json::json;
    use sha2::Digest;

    let invocation = |invocation: &Invocation| {
        let named: BTreeMap<_, _> = invocation
            .args
            .named
            .iter()
            .flatten()
            .map(|(key, value)| (key, value))
            .collect();
        json!({
            "method": invocation.method,
            "abi": invocation.abi,
            "named": named,
            "positional": invocation.args.positional.as_deref().unwrap_or_default(),
        })
    };
    let canonical = match step {
        Step::External(step) => json!({ "ids": step.ids }),
        Step::Contract(step) => {
            let source = match &step.source {
                Source::Package { .. } | Source::Wasm { .. } => json!({ "wasm": wasm_sha256 }),
                Source::Clone { of } => json!({ "clone": of }),
            };
            json!({ "source": source, "reserve": step.reserve, "init": invocation(&step.init), "tx": step.tx })
        }
        Step::Call(step) => json!({
            "contract": step.contract, "call": invocation(&step.call), "tx": step.tx
        }),
        Step::Setting(step) => json!({
            "contract": step.contract, "read": step.read, "write": step.write,
            "initializerArg": step.initializer_arg, "valueType": step.value_type, "tx": step.tx,
        }),
    };
    let payload = serde_json::to_vec(&(step.kind(), canonical)).expect("serializable definition");
    format!("v2:{}", hex::encode(sha2::Sha256::digest(payload)))
}

fn render_args(args: &Args) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(named) = &args.named {
        let mut pairs: Vec<String> = named
            .iter()
            .map(|(name, value)| format!("{name}={}", value.render()))
            .collect();
        // Sorted, so the order named args are written in never matters.
        pairs.sort();
        parts.extend(pairs);
    }
    if let Some(positional) = &args.positional {
        parts.extend(positional.iter().map(Value::render));
    }
    parts.join(",")
}

fn render_tx(tx: &TxShape) -> String {
    format!(
        "inputs={}|to={}|pointer={}|refund={}|edicts={}",
        tx.inputs
            .iter()
            .map(Value::render)
            .collect::<Vec<_>>()
            .join(","),
        tx.to.as_ref().map(Value::render).unwrap_or_default(),
        tx.pointer.as_deref().unwrap_or(""),
        tx.refund.as_deref().unwrap_or(""),
        tx.edicts
            .iter()
            .map(Value::render)
            .collect::<Vec<_>>()
            .join(","),
    )
}

/// Compare records written before definitions used structured encoding.
pub(crate) fn legacy_definition_hash(step: &Step, wasm_sha256: Option<&str>) -> String {
    use sha2::Digest;
    let canonical = match step {
        Step::External(step) => format!("external|{:?}", step.ids),
        Step::Contract(step) => {
            let source = match &step.source {
                Source::Package { .. } | Source::Wasm { .. } => {
                    format!("wasm:{}", wasm_sha256.unwrap_or(""))
                }
                Source::Clone { of } => format!("clone:{}", of.render()),
            };
            format!(
                "contract|{source}|reserve={}|init={}({})|{}",
                step.reserve.as_deref().unwrap_or(""),
                step.init.method.as_deref().unwrap_or(""),
                render_args(&step.init.args),
                render_tx(&step.tx),
            )
        }
        Step::Call(step) => format!(
            "call|{}|{}({})|abi={}|{}",
            step.contract.render(),
            step.call.method.as_deref().unwrap_or(""),
            render_args(&step.call.args),
            step.call.abi.as_deref().unwrap_or(""),
            render_tx(&step.tx),
        ),
        Step::Setting(step) => format!(
            "setting|{}|read={}|write={}|{:?}|{}",
            step.contract.render(),
            step.read.as_deref().unwrap_or(""),
            step.write.as_deref().unwrap_or(""),
            step.value_type,
            render_tx(&step.tx),
        ),
    };
    hex::encode(sha2::Sha256::digest(canonical.as_bytes()))
}

/// Parse and validate the graph a deployment script emitted for `network`.
pub fn parse(text: &str, network: &str) -> Result<Graph> {
    let graph: Graph = serde_json::from_str(text).map_err(|e| {
        invalid(format!(
            "deploy.rs emitted an invalid deployment graph: {e}"
        ))
    })?;
    if graph.schema != GRAPH_SCHEMA {
        return Err(LabcoatError::new(
            "DEPLOY_SCRIPT_INVALID",
            format!(
                "deploy.rs emitted graph schema `{}`, but this labcoat reads `{GRAPH_SCHEMA}`",
                graph.schema
            ),
            "run `labcoat upgrade` so the project's labcoat-deploy matches the installed CLI",
        ));
    }
    if graph.network != network {
        return Err(invalid(format!(
            "deploy.rs emitted a graph for network `{}`, expected `{network}`",
            graph.network
        )));
    }
    validate(&graph)?;
    Ok(graph)
}

fn validate(graph: &Graph) -> Result<()> {
    // name → (kind, lifecycle) of every step declared so far.
    let mut declared: BTreeMap<&str, &Step> = BTreeMap::new();
    let mut names = BTreeSet::new();
    for step in &graph.steps {
        let name = step.name();
        let valid_name = !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        if !valid_name {
            return Err(invalid(format!(
                "step name `{name}` must use only letters, digits, `_`, and `-`"
            )));
        }
        if !names.insert(name) {
            return Err(invalid(format!("duplicate step name `{name}`")));
        }
        let context = format!("{} `{name}`", step.kind());

        for reference in step.references() {
            let Some(target) = declared.get(reference.step.as_str()) else {
                return Err(invalid(format!(
                    "{context} references `{}`, which is not declared before it",
                    reference.step
                )));
            };
            match (target, reference.field) {
                (Step::External(_), Field::Id | Field::Block | Field::Tx) => {}
                (Step::Contract(_), _) => {}
                (other, _) => {
                    return Err(invalid(format!(
                        "{context} references {} `{}`, which has no such id",
                        other.kind(),
                        reference.step
                    )))
                }
            }
        }

        match step {
            Step::External(external) => {
                if external.ids.is_empty() {
                    return Err(invalid(format!(
                        "{context} has no id — bind one with `.on(network, block, tx)` or `.id_everywhere(block, tx)`"
                    )));
                }
            }
            Step::Contract(contract) => {
                let reserve = contract.reserve_number()?;
                if reserve.is_some() && matches!(contract.source, Source::Clone { .. }) {
                    return Err(invalid(format!(
                        "{context} is a clone and cannot take a reserve — clones always get the next free id"
                    )));
                }
                if reserve.is_some() && contract.lifecycle == Lifecycle::Replaceable {
                    return Err(invalid(format!(
                        "{context} is an implementation and cannot take a reserve — each new instance needs a fresh id"
                    )));
                }
                validate_args(&context, &contract.init)?;
            }
            Step::Call(call) => {
                if call.call.method.as_deref().is_none_or(str::is_empty) {
                    return Err(invalid(format!("{context} has no method")));
                }
                validate_args(&context, &call.call)?;
                if let Some(abi) = &call.call.abi {
                    if !matches!(declared.get(abi.as_str()), Some(Step::Contract(_))) {
                        return Err(invalid(format!(
                            "{context} resolves its ABI from `{abi}`, which is not a contract declared before it"
                        )));
                    }
                }
            }
            Step::Setting(setting) => {
                if setting.write.as_deref().is_none_or(str::is_empty) {
                    return Err(invalid(format!(
                        "{context} has no write method — set one with `.write(method)`"
                    )));
                }
                if let Some(arg) = &setting.initializer_arg {
                    let valid = match &setting.contract {
                        Value::Ref(reference) if reference.field == Field::Id => {
                            match declared.get(reference.step.as_str()) {
                                Some(Step::Contract(contract)) => contract
                                    .init
                                    .args
                                    .named
                                    .iter()
                                    .flatten()
                                    .any(|(name, _)| name == arg),
                                _ => false,
                            }
                        }
                        _ => false,
                    };
                    if !valid {
                        return Err(invalid(format!(
                            "{context} uses initializer argument `{arg}` missing from its target"
                        )));
                    }
                }
            }
        }
        declared.insert(name, step);
    }
    Ok(())
}

fn validate_args(context: &str, invocation: &Invocation) -> Result<()> {
    let args = &invocation.args;
    if args.named.is_some() && args.positional.is_some() {
        return Err(invalid(format!(
            "{context} mixes named and positional args"
        )));
    }
    if let Some(named) = &args.named {
        let numeric = invocation
            .method
            .as_deref()
            .is_some_and(|method| method.parse::<u128>().is_ok());
        if numeric && !named.is_empty() {
            return Err(invalid(format!(
                "{context} passes named args to a numeric opcode — use `.positional(value)` with raw cellpack values, or an ABI method name"
            )));
        }
        let mut seen = BTreeSet::new();
        for (name, _) in named {
            if !seen.insert(name) {
                return Err(invalid(format!("{context} repeats named arg `{name}`")));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(steps: &str) -> String {
        format!(r#"{{"schema":"{GRAPH_SCHEMA}","network":"labcoat","steps":[{steps}]}}"#)
    }

    const TOKEN: &str = r#"{"kind":"contract","name":"token","source":{"type":"package","package":"token"},"lifecycle":"pinned","init":{"args":{"named":[["supply",{"lit":"100"}]]}},"tx":{}}"#;

    #[test]
    fn parses_steps_in_declaration_order() {
        let text = graph(&format!(
            r#"{{"kind":"external","name":"usd","ids":{{"labcoat":[4,65012]}}}},{TOKEN},
            {{"kind":"call","name":"fund","contract":{{"ref":{{"step":"token","field":"id"}}}},"call":{{"method":"fund","args":{{}}}},"tx":{{"inputs":[{{"concat":[{{"ref":{{"step":"token","field":"id"}}}},{{"lit":":100"}}]}}]}}}}"#
        ));
        let graph = parse(&text, "labcoat").unwrap();
        let names: Vec<&str> = graph.steps.iter().map(Step::name).collect();
        assert_eq!(names, ["usd", "token", "fund"]);
        assert_eq!(graph.steps[2].references().len(), 2);
    }

    #[test]
    fn rejects_forward_and_unknown_references() {
        let text = graph(
            r#"{"kind":"call","name":"fund","contract":{"ref":{"step":"token","field":"id"}},"call":{"method":"fund","args":{}},"tx":{}}"#,
        );
        let error = parse(&text, "labcoat").unwrap_err();
        assert_eq!(error.code, "DEPLOY_SCRIPT_INVALID");
        assert!(error.message.contains("not declared before it"));
    }

    #[test]
    fn rejects_a_graph_for_another_network_or_schema() {
        let error = parse(&graph(TOKEN), "signet").unwrap_err();
        assert!(error.message.contains("expected `signet`"));
        let other = graph(TOKEN).replace(GRAPH_SCHEMA, "labcoat/deployment/v9");
        assert!(parse(&other, "labcoat")
            .unwrap_err()
            .message
            .contains("graph schema"));
    }

    #[test]
    fn rejects_duplicates_and_reserved_implementations() {
        let error = parse(&graph(&format!("{TOKEN},{TOKEN}")), "labcoat").unwrap_err();
        assert!(error.message.contains("duplicate step name `token`"));

        let reserved = TOKEN.replace(
            r#""lifecycle":"pinned""#,
            r#""lifecycle":"replaceable","reserve":"7""#,
        );
        let error = parse(&graph(&reserved), "labcoat").unwrap_err();
        assert!(error.message.contains("cannot take a reserve"));
    }

    #[test]
    fn definition_hash_ignores_named_arg_order_and_tracks_the_artifact() {
        let a: Step = serde_json::from_str(
            r#"{"kind":"contract","name":"t","source":{"type":"package","package":"t"},"lifecycle":"pinned","init":{"args":{"named":[["a",{"lit":"1"}],["b",{"height":{"offset":100}}]]}}}"#,
        )
        .unwrap();
        let b: Step = serde_json::from_str(
            r#"{"kind":"contract","name":"t","source":{"type":"package","package":"t"},"lifecycle":"pinned","init":{"args":{"named":[["b",{"height":{"offset":100}}],["a",{"lit":"1"}]]}}}"#,
        )
        .unwrap();
        assert_eq!(
            definition_hash(&a, Some("aa")),
            definition_hash(&b, Some("aa"))
        );
        assert_ne!(
            definition_hash(&a, Some("aa")),
            definition_hash(&a, Some("bb"))
        );
    }

    #[test]
    fn evaluates_references_heights_and_created_ids() {
        let mut resolved = Resolved::default();
        resolved
            .insert("beacon", "2:7", &["2:7".into(), "2:8".into()])
            .unwrap();
        let id = Value::Ref(Reference {
            step: "beacon".into(),
            field: Field::Id,
            index: None,
        });
        let auth = Value::Concat(vec![
            Value::Ref(Reference {
                step: "beacon".into(),
                field: Field::Created,
                index: Some(1),
            }),
            Value::Lit(":1".into()),
        ]);
        let pending = Value::Ref(Reference {
            step: "later".into(),
            field: Field::Tx,
            index: None,
        });
        assert_eq!(resolved.eval(&id, 10).as_deref(), Some("2:7"));
        assert_eq!(resolved.eval(&auth, 10).as_deref(), Some("2:8:1"));
        assert_eq!(
            resolved
                .eval(&Value::Height(HeightOffset { offset: 100 }), 10)
                .as_deref(),
            Some("110")
        );
        assert_eq!(resolved.eval(&pending, 10), None);
        assert_eq!(resolved.preview(&pending, 10), "<later.tx>");
    }
}
