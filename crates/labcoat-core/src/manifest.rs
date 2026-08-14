//! The HCL deployment manifest (`alkanes.hcl`) — declarative deployment
//! topology applied by `labcoat plan` / `labcoat apply`.
//!
//! ```hcl
//! # An external Alkane: binds a name to an id that exists outside this
//! # deployment. Never deployed, never a dependency edge. The id is either
//! # one binding for every network, or a per-network map — plan fails fast
//! # when the active network has no binding.
//! alkane "usd" {
//!   id = {
//!     labcoat = [4, 65012]
//!     signet  = [2, 190213]
//!   }
//! }
//!
//! # A managed contract: this manifest owns and deploys it.
//! contract "token" {
//!   package = "strata_test_token"
//!   reserve = 65011
//!   args    = [1, 100]
//! }
//!
//! contract "series" {
//!   package = "strata_series"
//!   reserve = 65014
//!   # Named args are matched to the ABI constructor's parameter names.
//!   args = {
//!     underlying = contract.token.id
//!     quote      = alkane.usd.id
//!     strike     = 75
//!     expiry     = height + 100
//!     supply     = 100
//!   }
//! }
//!
//! call "fund_series" {
//!   contract = "series"
//!   method   = "fund"
//!   inputs   = ["${contract.token.id}:100"]
//! }
//! ```
//!
//! References are namespaced: `alkane.<name>.<field>` resolves immediately
//! and creates no deployment edge, while `contract.<name>.<field>` resolves
//! to a managed deployment's id and orders that deployment first. Fields are
//! `id` (the `"block:tx"` string), `block`, and `tx`.
//!
//! Scope guard: the manifest is deliberately not a programming language.
//! Expressions are limited to literals, references, `height`, arithmetic,
//! and string templates. Function calls, conditionals, `for` expressions,
//! and splats are rejected at parse time — anything conditional belongs in
//! Rust tests, not here.

use crate::error::{LabcoatError, Result};
use hcl::expr::{Expression, ObjectKey, Operation, Traversal, TraversalOperator};
use hcl::structure::{Block, Body};
use hcl::template::{Element, Template};
use std::collections::{BTreeMap, BTreeSet};

pub const MANIFEST: &str = "alkanes.hcl";

/// The variable holding the chain height at apply time.
const HEIGHT: &str = "height";
/// Namespace for external Alkane references.
const NS_ALKANE: &str = "alkane";
/// Namespace for managed contract references.
const NS_CONTRACT: &str = "contract";

#[derive(Debug, Clone)]
pub struct Manifest {
    /// External Alkane references, declaration order.
    pub alkanes: Vec<AlkaneEntry>,
    /// Declaration order (deploy order is the reference topology, with
    /// declaration order as the tie-break).
    pub contracts: Vec<ContractEntry>,
    /// Declaration order (executed after every contract).
    pub calls: Vec<CallEntry>,
}

/// An `alkane "name" { id = ... }` block: a symbolic name for an Alkane
/// that exists outside this deployment.
#[derive(Debug, Clone)]
pub struct AlkaneEntry {
    pub name: String,
    pub ids: AlkaneIds,
}

/// An alkane's id bindings: one id valid on every network, or a map of
/// per-network ids (keys validated against the known network targets).
#[derive(Debug, Clone)]
pub enum AlkaneIds {
    All(u64, u64),
    PerNetwork(BTreeMap<String, (u64, u64)>),
}

impl AlkaneEntry {
    /// The id bound for `network`, or None when a per-network map has no
    /// entry for it.
    pub fn id_for(&self, network: &str) -> Option<(u64, u64)> {
        match &self.ids {
            AlkaneIds::All(block, tx) => Some((*block, *tx)),
            AlkaneIds::PerNetwork(map) => map.get(network).copied(),
        }
    }
}

/// Constructor or call arguments: positional (encoded in order) or named
/// (matched to the ABI's parameter names).
#[derive(Debug, Clone)]
pub enum Args {
    Positional(Vec<Expression>),
    Named(Vec<(String, Expression)>),
}

impl Default for Args {
    fn default() -> Self {
        Args::Positional(Vec::new())
    }
}

impl Args {
    pub fn is_empty(&self) -> bool {
        match self {
            Args::Positional(items) => items.is_empty(),
            Args::Named(items) => items.is_empty(),
        }
    }

    pub fn exprs(&self) -> Box<dyn Iterator<Item = &Expression> + '_> {
        match self {
            Args::Positional(items) => Box::new(items.iter()),
            Args::Named(items) => Box::new(items.iter().map(|(_, expr)| expr)),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ContractEntry {
    pub name: String,
    pub package: Option<String>,
    pub wasm: Option<String>,
    pub reserve: Option<u128>,
    pub args: Args,
}

impl ContractEntry {
    /// Every expression-bearing field, so dependency collection and
    /// reference validation pick up future fields automatically.
    pub fn referenced_exprs(&self) -> Box<dyn Iterator<Item = &Expression> + '_> {
        self.args.exprs()
    }
}

#[derive(Debug, Clone)]
pub struct CallEntry {
    pub label: String,
    pub contract: Expression,
    pub method: String,
    pub args: Args,
    pub inputs: Vec<Expression>,
    pub to: Option<Expression>,
    pub pointer: Option<String>,
    pub refund: Option<String>,
    pub edicts: Vec<Expression>,
}

impl CallEntry {
    /// Every expression-bearing field, so dependency collection and
    /// reference validation pick up future fields automatically.
    pub fn referenced_exprs(&self) -> Box<dyn Iterator<Item = &Expression> + '_> {
        Box::new(
            std::iter::once(&self.contract)
                .chain(self.args.exprs())
                .chain(self.inputs.iter())
                .chain(self.to.iter())
                .chain(self.edicts.iter()),
        )
    }
}

fn invalid(message: impl Into<String>) -> LabcoatError {
    LabcoatError::new(
        "MANIFEST_INVALID",
        message.into(),
        "alkane blocks declare id; contract blocks declare package|wasm, reserve, args; call blocks declare contract, method, args, inputs, to, pointer, refund, edicts; references are alkane.<name>.<field> / contract.<name>.<field>",
    )
}

// ---------------------------------------------------------------------------
// References

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RefKind {
    Alkane,
    Contract,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RefField {
    Id,
    Block,
    Tx,
}

/// A namespaced reference: `alkane.<name>.<field>` or `contract.<name>.<field>`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Ref {
    pub kind: RefKind,
    pub name: String,
    pub field: RefField,
}

/// Collect the namespaced references an expression makes, rejecting the
/// control-flow constructs the manifest deliberately excludes.
pub fn expression_refs(expr: &Expression, refs: &mut BTreeSet<Ref>) -> Result<()> {
    match expr {
        Expression::Null
        | Expression::Bool(_)
        | Expression::Number(_)
        | Expression::String(_) => Ok(()),
        Expression::Array(items) => {
            for item in items {
                expression_refs(item, refs)?;
            }
            Ok(())
        }
        Expression::Object(object) => {
            for value in object.values() {
                expression_refs(value, refs)?;
            }
            Ok(())
        }
        Expression::Variable(variable) => match variable.as_str() {
            HEIGHT => Ok(()),
            ns @ (NS_ALKANE | NS_CONTRACT) => Err(invalid(format!(
                "`{ns}` is a namespace — write `{ns}.<name>.id` (or .block/.tx)"
            ))),
            other => Err(invalid(format!(
                "unknown variable `{other}` — only `height`, `alkane.<name>.<field>`, and `contract.<name>.<field>` are available in the manifest"
            ))),
        },
        Expression::Traversal(traversal) => traversal_ref(traversal, refs),
        Expression::TemplateExpr(template) => {
            let template = Template::from_expr(template)
                .map_err(|e| invalid(format!("bad template expression: {e}")))?;
            for element in template.elements() {
                match element {
                    Element::Literal(_) => {}
                    Element::Interpolation(interpolation) => {
                        expression_refs(&interpolation.expr, refs)?
                    }
                    Element::Directive(_) => {
                        return Err(invalid(
                            "template directives (%{if}/%{for}) are not supported in the manifest — move conditional logic into Rust tests",
                        ))
                    }
                }
            }
            Ok(())
        }
        Expression::Parenthesis(inner) => expression_refs(inner, refs),
        Expression::Operation(operation) => match operation.as_ref() {
            Operation::Unary(op) => expression_refs(&op.expr, refs),
            Operation::Binary(op) => {
                expression_refs(&op.lhs_expr, refs)?;
                expression_refs(&op.rhs_expr, refs)
            }
        },
        Expression::FuncCall(call) => Err(invalid(format!(
            "function calls are not supported in the manifest (found `{}`)",
            call.name
        ))),
        Expression::Conditional(_) => Err(invalid(
            "conditional expressions are not supported in the manifest — move conditional logic into Rust tests",
        )),
        Expression::ForExpr(_) => Err(invalid(
            "for expressions are not supported in the manifest — move loops into Rust tests",
        )),
        other => Err(invalid(format!(
            "unsupported expression `{}` in the manifest",
            render(other)
        ))),
    }
}

fn traversal_ref(traversal: &Traversal, refs: &mut BTreeSet<Ref>) -> Result<()> {
    let text = render(&Expression::Traversal(Box::new(traversal.clone())));
    let kind = match &traversal.expr {
        Expression::Variable(variable) => match variable.as_str() {
            NS_ALKANE => RefKind::Alkane,
            NS_CONTRACT => RefKind::Contract,
            HEIGHT => {
                return Err(invalid(format!(
                    "`{HEIGHT}` is a plain number and has no fields (found `{text}`)"
                )))
            }
            _ => {
                return Err(invalid(format!(
                    "unknown reference `{text}` — reference managed contracts as `contract.{text}` and external alkanes as `alkane.{text}`"
                )))
            }
        },
        _ => {
            return Err(invalid(format!(
                "unsupported reference `{text}` — references take the form `alkane.<name>.<field>` or `contract.<name>.<field>`"
            )))
        }
    };
    let ns = match kind {
        RefKind::Alkane => NS_ALKANE,
        RefKind::Contract => NS_CONTRACT,
    };
    let (name, field) = match traversal.operators.as_slice() {
        [TraversalOperator::GetAttr(name), TraversalOperator::GetAttr(field)] => (name, field),
        _ => {
            return Err(invalid(format!(
                "`{ns}` references take the form `{ns}.<name>.<field>` with field id, block, or tx (found `{text}`)"
            )))
        }
    };
    let field = match field.as_str() {
        "id" => RefField::Id,
        "block" => RefField::Block,
        "tx" => RefField::Tx,
        other => {
            return Err(invalid(format!(
                "unknown field `{other}` on `{ns}.{name}` — available fields: id, block, tx",
                name = name.as_str()
            )))
        }
    };
    refs.insert(Ref {
        kind,
        name: name.as_str().to_string(),
        field,
    });
    Ok(())
}

fn validated_refs(expr: &Expression) -> Result<BTreeSet<Ref>> {
    let mut refs = BTreeSet::new();
    expression_refs(expr, &mut refs)?;
    Ok(refs)
}

// ---------------------------------------------------------------------------
// Evaluation

/// Ids visible to manifest expressions: alkanes are seeded from the manifest
/// and always resolved; contracts are inserted as they deploy (or are read
/// from the lockfile / pre-seeded reserves).
#[derive(Debug, Clone, Default)]
pub struct ResolvedIds {
    alkanes: BTreeMap<String, (u64, u64)>,
    contracts: BTreeMap<String, (u64, u64)>,
}

impl ResolvedIds {
    /// Seed the alkane bindings for the active network. Errors when a
    /// per-network alkane has no binding for it — plan fails fast instead
    /// of applying a manifest to a network it was never bound for.
    pub fn from_manifest(manifest: &Manifest, network: &str) -> Result<Self> {
        let mut ids = Self::default();
        for alkane in &manifest.alkanes {
            let Some((block, tx)) = alkane.id_for(network) else {
                let bound = match &alkane.ids {
                    AlkaneIds::PerNetwork(map) => {
                        map.keys().cloned().collect::<Vec<_>>().join(", ")
                    }
                    AlkaneIds::All(..) => unreachable!("All ids bind every network"),
                };
                return Err(invalid(format!(
                    "alkane \"{}\" has no id for network `{network}` (bound: {bound}) — add a `{network}` entry to its id map",
                    alkane.name
                )));
            };
            ids.alkanes.insert(alkane.name.clone(), (block, tx));
        }
        Ok(ids)
    }

    /// Record a managed contract's id (`"block:tx"`). Errors when a part
    /// does not fit u64 — hcl numbers cap at u64, so larger ids cannot
    /// participate in manifest expressions.
    pub fn insert_contract(&mut self, name: &str, id: &str) -> Result<()> {
        let parts: Vec<&str> = id.split(':').collect();
        let parsed = match parts.as_slice() {
            [block, tx] => block
                .parse::<u64>()
                .and_then(|b| tx.parse::<u64>().map(|t| (b, t)))
                .ok(),
            _ => None,
        };
        let Some(pair) = parsed else {
            return Err(invalid(format!(
                "contract `{name}` id `{id}` cannot be used in manifest expressions — expected `block:tx` with each part fitting u64"
            )));
        };
        self.contracts.insert(name.to_string(), pair);
        Ok(())
    }

    pub fn contract_id(&self, name: &str) -> Option<String> {
        self.contracts
            .get(name)
            .map(|(block, tx)| format!("{block}:{tx}"))
    }

    pub fn has_contract(&self, name: &str) -> bool {
        self.contracts.contains_key(name)
    }

    fn namespace_value(entries: &BTreeMap<String, (u64, u64)>) -> hcl::Value {
        let mut ns = hcl::Map::new();
        for (name, (block, tx)) in entries {
            let mut object = hcl::Map::new();
            object.insert("id".to_string(), hcl::Value::from(format!("{block}:{tx}")));
            object.insert("block".to_string(), hcl::Value::from(*block));
            object.insert("tx".to_string(), hcl::Value::from(*tx));
            ns.insert(name.clone(), hcl::Value::from(object));
        }
        hcl::Value::from(ns)
    }
}

/// Evaluate an expression to its canonical scalar string (numbers keep
/// their decimal form; strings pass through). Returns None while any
/// referenced contract id is still pending.
pub fn eval_scalar(
    expr: &Expression,
    resolved: &ResolvedIds,
    height: u64,
) -> Result<Option<String>> {
    for reference in validated_refs(expr)? {
        match reference.kind {
            RefKind::Contract if !resolved.contracts.contains_key(&reference.name) => {
                return Ok(None)
            }
            RefKind::Alkane if !resolved.alkanes.contains_key(&reference.name) => {
                return Err(invalid(format!(
                    "`alkane.{}` is not declared — add an `alkane \"{}\" {{ id = [block, tx] }}` block",
                    reference.name, reference.name
                )))
            }
            _ => {}
        }
    }
    let mut ctx = hcl::eval::Context::new();
    ctx.declare_var(HEIGHT, height);
    ctx.declare_var(NS_ALKANE, ResolvedIds::namespace_value(&resolved.alkanes));
    ctx.declare_var(
        NS_CONTRACT,
        ResolvedIds::namespace_value(&resolved.contracts),
    );
    use hcl::eval::Evaluate;
    let value = expr
        .evaluate(&ctx)
        .map_err(|e| invalid(format!("cannot evaluate `{}`: {e}", render(expr))))?;
    scalar_value(expr, &value).map(Some)
}

fn scalar_value(expr: &Expression, value: &hcl::Value) -> Result<String> {
    match value {
        hcl::Value::String(s) => Ok(s.clone()),
        hcl::Value::Number(n) => {
            if let Some(u) = n.as_u64() {
                return Ok(u.to_string());
            }
            Err(invalid(format!(
                "`{}` evaluates to {n}, expected a non-negative integer or string",
                render(expr)
            )))
        }
        other => Err(invalid(format!(
            "`{}` evaluates to {other}, expected a non-negative integer or string",
            render(expr)
        ))),
    }
}

/// The expression's source-ish text, for previews and errors.
pub fn render(expr: &Expression) -> String {
    hcl::format::to_string(expr).unwrap_or_else(|_| format!("{expr:?}"))
}

// ---------------------------------------------------------------------------
// Parsing

/// A literal string attribute (no references).
fn literal_string(block: &str, key: &str, expr: &Expression) -> Result<String> {
    match expr {
        Expression::String(s) => Ok(s.clone()),
        other => Err(invalid(format!(
            "{block}.{key}: expected a literal string, found `{}`",
            render(other)
        ))),
    }
}

/// A literal non-negative integer attribute (no references).
fn literal_u128(block: &str, key: &str, expr: &Expression) -> Result<u128> {
    match expr {
        Expression::Number(n) => n
            .as_u64()
            .map(u128::from)
            .ok_or_else(|| invalid(format!("{block}.{key}: expected a non-negative integer"))),
        Expression::String(s) => s
            .parse()
            .map_err(|_| invalid(format!("{block}.{key}: expected a decimal u128"))),
        other => Err(invalid(format!(
            "{block}.{key}: expected an integer, found `{}`",
            render(other)
        ))),
    }
}

/// A literal `[block, tx]` id pair.
fn literal_id_pair(context: &str, expr: &Expression) -> Result<(u64, u64)> {
    let bad = || {
        invalid(format!(
            "{context}: id must be an array of two non-negative integers, e.g. id = [4, 65011]"
        ))
    };
    let Expression::Array(items) = expr else {
        return Err(bad());
    };
    let [block, tx] = items.as_slice() else {
        return Err(bad());
    };
    let part = |item: &Expression| match item {
        Expression::Number(n) => n.as_u64().ok_or_else(bad),
        _ => Err(bad()),
    };
    Ok((part(block)?, part(tx)?))
}

/// An alkane's `id` attribute: a `[block, tx]` pair binding every network,
/// or a `{ network = [block, tx], ... }` map of per-network bindings.
fn literal_alkane_ids(context: &str, expr: &Expression) -> Result<AlkaneIds> {
    use std::str::FromStr;

    match expr {
        Expression::Array(_) => {
            literal_id_pair(context, expr).map(|(block, tx)| AlkaneIds::All(block, tx))
        }
        Expression::Object(object) => {
            let mut map = BTreeMap::new();
            for (object_key, value) in object.iter() {
                let key = match object_key {
                    ObjectKey::Identifier(ident) => ident.as_str().to_string(),
                    ObjectKey::Expression(Expression::String(s)) => s.clone(),
                    other => {
                        return Err(invalid(format!(
                            "{context}: id map keys must be network names, found `{other}`"
                        )))
                    }
                };
                let network = crate::system::NetworkTarget::from_str(&key)
                    .map_err(|e| invalid(format!("{context}: id map: {e}")))?;
                let pair = literal_id_pair(context, value)?;
                if map.insert(network.id().to_string(), pair).is_some() {
                    return Err(invalid(format!(
                        "{context}: duplicate network `{}` in id map",
                        network.id()
                    )));
                }
            }
            if map.is_empty() {
                return Err(invalid(format!(
                    "{context}: id map must bind at least one network"
                )));
            }
            Ok(AlkaneIds::PerNetwork(map))
        }
        other => Err(invalid(format!(
            "{context}: id must be an array of two non-negative integers (id = [4, 65011]) or a per-network map (id = {{ regtest = [4, 65011] }}), found `{}`",
            render(other)
        ))),
    }
}

fn expression_list(block: &str, key: &str, expr: &Expression) -> Result<Vec<Expression>> {
    match expr {
        Expression::Array(items) => {
            for item in items {
                validated_refs(item)?;
            }
            Ok(items.clone())
        }
        other => Err(invalid(format!(
            "{block}.{key}: expected an array, found `{}`",
            render(other)
        ))),
    }
}

/// The `args` attribute: a positional array or an object of named args.
fn expression_args(block: &str, key: &str, expr: &Expression) -> Result<Args> {
    match expr {
        Expression::Array(items) => {
            for item in items {
                validated_refs(item)?;
            }
            Ok(Args::Positional(items.clone()))
        }
        Expression::Object(object) => {
            let mut named = Vec::new();
            let mut seen = BTreeSet::new();
            for (object_key, value) in object.iter() {
                let name = match object_key {
                    ObjectKey::Identifier(ident) => ident.as_str().to_string(),
                    ObjectKey::Expression(Expression::String(s)) => s.clone(),
                    ObjectKey::Expression(other) => {
                        return Err(invalid(format!(
                            "{block}.{key}: arg names must be identifiers, found `{}`",
                            render(other)
                        )))
                    }
                    other => {
                        return Err(invalid(format!(
                            "{block}.{key}: arg names must be identifiers, found `{other}`"
                        )))
                    }
                };
                if !seen.insert(name.clone()) {
                    return Err(invalid(format!("{block}.{key}: duplicate arg `{name}`")));
                }
                validated_refs(value)?;
                named.push((name, value.clone()));
            }
            Ok(Args::Named(named))
        }
        other => Err(invalid(format!(
            "{block}.{key}: expected an array of positional args or an object of named args, found `{}`",
            render(other)
        ))),
    }
}

fn block_attributes<'a>(
    context: &str,
    block: &'a Block,
) -> Result<BTreeMap<&'a str, &'a Expression>> {
    let mut attributes = BTreeMap::new();
    for structure in block.body.iter() {
        match structure {
            hcl::structure::Structure::Attribute(attribute) => {
                if attributes
                    .insert(attribute.key.as_str(), &attribute.expr)
                    .is_some()
                {
                    return Err(invalid(format!(
                        "{context}: duplicate attribute `{}`",
                        attribute.key.as_str()
                    )));
                }
            }
            hcl::structure::Structure::Block(inner) => {
                return Err(invalid(format!(
                    "{context}: nested `{}` blocks are not supported",
                    inner.identifier.as_str()
                )))
            }
        }
    }
    Ok(attributes)
}

fn block_label(block: &Block) -> Result<String> {
    let labels = &block.labels;
    if labels.len() != 1 {
        return Err(invalid(format!(
            "`{}` blocks take exactly one label, e.g. `{} \"name\" {{ ... }}`",
            block.identifier.as_str(),
            block.identifier.as_str()
        )));
    }
    let name = labels[0].as_str().to_string();
    if name.is_empty() {
        return Err(invalid(format!(
            "`{}` block label must not be empty",
            block.identifier.as_str()
        )));
    }
    if name == HEIGHT {
        return Err(invalid(format!(
            "{} name `{HEIGHT}` is reserved for the chain-height variable",
            block.identifier.as_str()
        )));
    }
    Ok(name)
}

fn parse_alkane(block: &Block) -> Result<AlkaneEntry> {
    let name = block_label(block)?;
    let context = format!("alkane \"{name}\"");
    let attributes = block_attributes(&context, block)?;
    let mut ids = None;
    for (key, expr) in &attributes {
        match *key {
            "id" => ids = Some(literal_alkane_ids(&context, expr)?),
            other => return Err(invalid(format!("{context}: unknown attribute `{other}`"))),
        }
    }
    let ids = ids.ok_or_else(|| {
        invalid(format!(
            "{context}: missing required attribute `id`, e.g. id = [4, 65011]"
        ))
    })?;
    Ok(AlkaneEntry { name, ids })
}

fn parse_contract(block: &Block) -> Result<ContractEntry> {
    let name = block_label(block)?;
    let context = format!("contract \"{name}\"");
    let attributes = block_attributes(&context, block)?;
    let mut package = None;
    let mut wasm = None;
    let mut reserve = None;
    let mut args = Args::default();
    for (key, expr) in &attributes {
        match *key {
            "package" => package = Some(literal_string(&context, key, expr)?),
            "wasm" => wasm = Some(literal_string(&context, key, expr)?),
            "reserve" => reserve = Some(literal_u128(&context, key, expr)?),
            "args" => args = expression_args(&context, key, expr)?,
            other => return Err(invalid(format!("{context}: unknown attribute `{other}`"))),
        }
    }
    match (&package, &wasm) {
        (Some(_), Some(_)) => {
            return Err(invalid(format!(
                "{context}: declare either package or wasm, not both"
            )))
        }
        (None, None) => {
            return Err(invalid(format!(
                "{context}: declare a Cargo package or a wasm path"
            )))
        }
        _ => {}
    }
    Ok(ContractEntry {
        name,
        package,
        wasm,
        reserve,
        args,
    })
}

fn parse_call(block: &Block) -> Result<CallEntry> {
    let label = block_label(block)?;
    let context = format!("call \"{label}\"");
    let attributes = block_attributes(&context, block)?;
    let mut contract = None;
    let mut method = None;
    let mut args = Args::default();
    let mut inputs = Vec::new();
    let mut to = None;
    let mut pointer = None;
    let mut refund = None;
    let mut edicts = Vec::new();
    for (key, expr) in &attributes {
        match *key {
            "contract" => {
                validated_refs(expr)?;
                contract = Some((*expr).clone());
            }
            "method" => method = Some(literal_string(&context, key, expr)?),
            "args" => args = expression_args(&context, key, expr)?,
            "inputs" => inputs = expression_list(&context, key, expr)?,
            "to" => {
                validated_refs(expr)?;
                to = Some((*expr).clone());
            }
            "pointer" => pointer = Some(literal_string(&context, key, expr)?),
            "refund" => refund = Some(literal_string(&context, key, expr)?),
            "edicts" => edicts = expression_list(&context, key, expr)?,
            other => return Err(invalid(format!("{context}: unknown attribute `{other}`"))),
        }
    }
    Ok(CallEntry {
        label,
        contract: contract
            .ok_or_else(|| invalid(format!("{context}: missing required attribute `contract`")))?,
        method: method
            .ok_or_else(|| invalid(format!("{context}: missing required attribute `method`")))?,
        args,
        inputs,
        to,
        pointer,
        refund,
        edicts,
    })
}

pub fn parse(text: &str) -> Result<Manifest> {
    let body: Body =
        hcl::parse(text).map_err(|e| invalid(format!("cannot parse manifest: {e}")))?;

    let mut alkanes: Vec<AlkaneEntry> = Vec::new();
    let mut contracts: Vec<ContractEntry> = Vec::new();
    let mut calls: Vec<CallEntry> = Vec::new();
    let mut labels = BTreeSet::new();

    for structure in body.iter() {
        match structure {
            hcl::structure::Structure::Attribute(attribute) => {
                return Err(invalid(format!(
                    "top-level attribute `{}` is not supported — declare alkane, contract, and call blocks",
                    attribute.key.as_str()
                )))
            }
            hcl::structure::Structure::Block(block) => match block.identifier.as_str() {
                "alkane" => {
                    let entry = parse_alkane(block)?;
                    if !labels.insert(format!("alkane:{}", entry.name)) {
                        return Err(invalid(format!("duplicate alkane \"{}\"", entry.name)));
                    }
                    alkanes.push(entry);
                }
                "contract" => {
                    let entry = parse_contract(block)?;
                    if !labels.insert(format!("contract:{}", entry.name)) {
                        return Err(invalid(format!("duplicate contract \"{}\"", entry.name)));
                    }
                    contracts.push(entry);
                }
                "call" => {
                    let entry = parse_call(block)?;
                    if !labels.insert(format!("call:{}", entry.label)) {
                        return Err(invalid(format!("duplicate call \"{}\"", entry.label)));
                    }
                    calls.push(entry);
                }
                other => {
                    return Err(invalid(format!(
                    "unknown block `{other}` — the manifest declares `alkane`, `contract`, and `call` blocks"
                )))
                }
            },
        }
    }

    let manifest = Manifest {
        alkanes,
        contracts,
        calls,
    };
    validate_references(&manifest)?;
    deploy_order(&manifest)?;
    Ok(manifest)
}

/// Every reference must name a declared alkane or contract, so typos fail
/// at parse time instead of surfacing as unresolved ids mid-apply.
fn validate_references(manifest: &Manifest) -> Result<()> {
    let alkanes: BTreeSet<&str> = manifest.alkanes.iter().map(|a| a.name.as_str()).collect();
    let contracts: BTreeSet<&str> = manifest.contracts.iter().map(|c| c.name.as_str()).collect();

    let check = |context: &str, expr: &Expression| -> Result<()> {
        for reference in validated_refs(expr)? {
            let name = reference.name.as_str();
            match reference.kind {
                RefKind::Alkane if !alkanes.contains(name) => {
                    let hint = if contracts.contains(name) {
                        format!(" (did you mean `contract.{name}`?)")
                    } else {
                        String::new()
                    };
                    return Err(invalid(format!(
                        "{context}: `alkane.{name}` is not declared — add an `alkane \"{name}\" {{ id = [block, tx] }}` block{hint}"
                    )));
                }
                RefKind::Contract if !contracts.contains(name) => {
                    let hint = if alkanes.contains(name) {
                        format!(" (did you mean `alkane.{name}`?)")
                    } else {
                        String::new()
                    };
                    return Err(invalid(format!(
                        "{context}: `contract.{name}` is not declared — declare a `contract \"{name}\"` block{hint}"
                    )));
                }
                _ => {}
            }
        }
        Ok(())
    };

    for entry in &manifest.contracts {
        let context = format!("contract \"{}\"", entry.name);
        for expr in entry.referenced_exprs() {
            check(&context, expr)?;
        }
    }
    for call in &manifest.calls {
        let context = format!("call \"{}\"", call.label);
        for expr in call.referenced_exprs() {
            check(&context, expr)?;
        }
        // The bare-name sugar `contract = "series"` must name a managed
        // contract (literal "block:tx" ids pass through untouched).
        if let Expression::String(target) = &call.contract {
            if !target.contains(':') && !contracts.contains(target.as_str()) {
                let hint = if alkanes.contains(target.as_str()) {
                    format!(" (for an external alkane, use contract = alkane.{target}.id)")
                } else {
                    String::new()
                };
                return Err(invalid(format!(
                    "{context}: contract \"{target}\" is not declared — declare a `contract \"{target}\"` block or use a literal \"block:tx\" id{hint}"
                )));
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Deploy order

/// Managed contracts referenced by an entry's expressions. Alkane
/// references are symbolic constants and never create deployment edges.
fn contract_dependencies(entry: &ContractEntry) -> Result<BTreeSet<String>> {
    let mut refs = BTreeSet::new();
    for expr in entry.referenced_exprs() {
        expression_refs(expr, &mut refs)?;
    }
    Ok(refs
        .into_iter()
        .filter(|r| r.kind == RefKind::Contract)
        .map(|r| r.name)
        .collect())
}

/// Deploy order: reference topology, declaration order as tie-break.
/// Rejects reference cycles and self-references.
pub fn deploy_order(manifest: &Manifest) -> Result<Vec<usize>> {
    let index: BTreeMap<&str, usize> = manifest
        .contracts
        .iter()
        .enumerate()
        .map(|(i, c)| (c.name.as_str(), i))
        .collect();

    let mut order = Vec::new();
    let mut done = BTreeSet::new();
    let mut in_progress = BTreeSet::new();

    fn visit(
        i: usize,
        manifest: &Manifest,
        index: &BTreeMap<&str, usize>,
        done: &mut BTreeSet<usize>,
        in_progress: &mut BTreeSet<usize>,
        order: &mut Vec<usize>,
    ) -> Result<()> {
        if done.contains(&i) {
            return Ok(());
        }
        if !in_progress.insert(i) {
            return Err(invalid(format!(
                "reference cycle through contract \"{}\"",
                manifest.contracts[i].name
            )));
        }
        for dep in contract_dependencies(&manifest.contracts[i])? {
            if let Some(&j) = index.get(dep.as_str()) {
                if j == i {
                    return Err(invalid(format!(
                        "contract \"{}\" references itself",
                        manifest.contracts[i].name
                    )));
                }
                visit(j, manifest, index, done, in_progress, order)?;
            }
        }
        in_progress.remove(&i);
        done.insert(i);
        order.push(i);
        Ok(())
    }

    for i in 0..manifest.contracts.len() {
        visit(i, manifest, &index, &mut done, &mut in_progress, &mut order)?;
    }
    Ok(order)
}

pub fn load(path: &std::path::Path) -> Result<Manifest> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        LabcoatError::new(
            "MANIFEST_INVALID",
            format!("cannot read {}: {e}", path.display()),
            "create the manifest or pass --manifest <path>",
        )
    })?;
    parse(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    const COVERED_CALL: &str = r#"
alkane "usd" {
  id = [4, 65012]
}

contract "fire" {
  package = "strata_test_token"
  reserve = 65011
  args    = [1, 100]
}

contract "series" {
  package = "strata_series"
  reserve = 65014
  args    = [contract.fire.id, alkane.usd.id, 75, height + 100, 100]
}

call "fund_series" {
  contract = "series"
  method   = "fund"
  inputs   = ["${contract.fire.id}:100"]
}
"#;

    fn resolved(manifest: &Manifest, contracts: &[(&str, &str)]) -> ResolvedIds {
        let mut ids = ResolvedIds::from_manifest(manifest, "labcoat").unwrap();
        for (name, id) in contracts {
            ids.insert_contract(name, id).unwrap();
        }
        ids
    }

    fn positional(args: &Args) -> &[Expression] {
        match args {
            Args::Positional(items) => items,
            Args::Named(_) => panic!("expected positional args"),
        }
    }

    #[test]
    fn parses_the_covered_call_topology_in_declaration_order() {
        let manifest = parse(COVERED_CALL).unwrap();
        let alkanes: Vec<_> = manifest.alkanes.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(alkanes, ["usd"]);
        assert_eq!(manifest.alkanes[0].id_for("labcoat"), Some((4, 65012)));
        let names: Vec<_> = manifest.contracts.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["fire", "series"]);
        assert_eq!(manifest.contracts[0].reserve, Some(65_011));
        assert_eq!(manifest.calls[0].label, "fund_series");
    }

    #[test]
    fn evaluates_references_heights_and_templates() {
        let manifest = parse(COVERED_CALL).unwrap();
        let ids = resolved(&manifest, &[("fire", "4:65011")]);

        let series_args: Vec<_> = positional(&manifest.contracts[1].args)
            .iter()
            .map(|e| eval_scalar(e, &ids, 423).unwrap().unwrap())
            .collect();
        assert_eq!(series_args, ["4:65011", "4:65012", "75", "523", "100"]);

        let input = eval_scalar(&manifest.calls[0].inputs[0], &ids, 423)
            .unwrap()
            .unwrap();
        assert_eq!(input, "4:65011:100");
    }

    #[test]
    fn evaluates_block_and_tx_fields() {
        let manifest = parse(
            r#"
alkane "usd" {
  id = [4, 65012]
}

contract "series" {
  package = "series"
  args    = [alkane.usd.block, alkane.usd.tx + 1, "${alkane.usd.block}:${alkane.usd.tx}"]
}
"#,
        )
        .unwrap();
        let ids = resolved(&manifest, &[]);
        let args: Vec<_> = positional(&manifest.contracts[0].args)
            .iter()
            .map(|e| eval_scalar(e, &ids, 0).unwrap().unwrap())
            .collect();
        assert_eq!(args, ["4", "65013", "4:65012"]);
    }

    #[test]
    fn pending_references_evaluate_to_none() {
        let manifest = parse(COVERED_CALL).unwrap();
        let ids = resolved(&manifest, &[]);
        let series_args = positional(&manifest.contracts[1].args);
        // contract.fire.id unresolved → the whole expression is pending.
        assert_eq!(eval_scalar(&series_args[0], &ids, 423).unwrap(), None);
        // alkane references resolve immediately.
        assert_eq!(
            eval_scalar(&series_args[1], &ids, 423).unwrap(),
            Some("4:65012".into())
        );
        // height-only expressions always resolve.
        assert_eq!(
            eval_scalar(&series_args[3], &ids, 423).unwrap(),
            Some("523".into())
        );
    }

    #[test]
    fn parses_named_args_in_declaration_order() {
        let manifest = parse(
            r#"
alkane "usd" {
  id = [4, 65012]
}

contract "series" {
  package = "series"
  args = {
    quote  = alkane.usd.id
    strike = 75
    expiry = height + 100
  }
}
"#,
        )
        .unwrap();
        let Args::Named(named) = &manifest.contracts[0].args else {
            panic!("expected named args");
        };
        let names: Vec<_> = named.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["quote", "strike", "expiry"]);
        let ids = resolved(&manifest, &[]);
        assert_eq!(
            eval_scalar(&named[0].1, &ids, 0).unwrap(),
            Some("4:65012".into())
        );
    }

    #[test]
    fn rejects_bad_named_args() {
        let duplicate = parse(
            "contract \"a\" {\n  package = \"a\"\n  args = { x = 1, \"x\" = 2 }\n}",
        )
        .unwrap_err();
        assert!(duplicate.message.contains("duplicate arg `x`"));

        let scalar = parse("contract \"a\" {\n  package = \"a\"\n  args = 1\n}").unwrap_err();
        assert!(scalar
            .message
            .contains("expected an array of positional args or an object of named args"));

        let computed_key =
            parse("contract \"a\" {\n  package = \"a\"\n  args = { (height) = 1 }\n}").unwrap_err();
        assert!(computed_key.message.contains("arg names must be identifiers"));
    }

    #[test]
    fn deploy_order_follows_references() {
        let manifest = parse(
            r#"
contract "series" {
  package = "series"
  args    = [contract.token.id]
}

contract "token" {
  package = "token"
}
"#,
        )
        .unwrap();
        let order = deploy_order(&manifest).unwrap();
        let names: Vec<_> = order
            .iter()
            .map(|&i| manifest.contracts[i].name.as_str())
            .collect();
        assert_eq!(names, ["token", "series"]);
    }

    #[test]
    fn alkane_references_create_no_deploy_edge() {
        let manifest = parse(
            r#"
alkane "token" {
  id = [4, 65011]
}

contract "series" {
  package = "series"
  args    = [alkane.token.id]
}

contract "other" {
  package = "other"
}
"#,
        )
        .unwrap();
        let order = deploy_order(&manifest).unwrap();
        let names: Vec<_> = order
            .iter()
            .map(|&i| manifest.contracts[i].name.as_str())
            .collect();
        // Declaration order — the alkane reference orders nothing.
        assert_eq!(names, ["series", "other"]);
    }

    #[test]
    fn alkane_and_contract_may_share_a_name() {
        let manifest = parse(
            r#"
alkane "token" {
  id = [4, 65011]
}

contract "token" {
  package = "token"
  args    = [alkane.token.id]
}
"#,
        )
        .unwrap();
        assert_eq!(manifest.alkanes[0].name, "token");
        assert_eq!(manifest.contracts[0].name, "token");
        // alkane.token.id is the external id, not a self-reference.
        deploy_order(&manifest).unwrap();
    }

    #[test]
    fn rejects_cycles_and_self_references() {
        let cycle = parse(
            r#"
contract "a" {
  package = "a"
  args    = [contract.b.id]
}

contract "b" {
  package = "b"
  args    = [contract.a.id]
}
"#,
        )
        .unwrap_err();
        assert_eq!(cycle.code, "MANIFEST_INVALID");
        assert!(cycle.message.contains("cycle"));

        let this =
            parse("contract \"a\" {\n  package = \"a\"\n  args = [contract.a.id]\n}").unwrap_err();
        assert!(this.message.contains("references itself"));
    }

    #[test]
    fn rejects_bare_and_unknown_references() {
        let bare = parse("contract \"a\" {\n  package = \"a\"\n  args = [fire.id]\n}").unwrap_err();
        assert!(bare.message.contains("`contract.fire.id`"), "{}", bare.message);
        assert!(bare.message.contains("`alkane.fire.id`"), "{}", bare.message);

        let variable =
            parse("contract \"a\" {\n  package = \"a\"\n  args = [supply]\n}").unwrap_err();
        assert!(variable.message.contains("unknown variable `supply`"));

        let namespace =
            parse("contract \"a\" {\n  package = \"a\"\n  args = [alkane]\n}").unwrap_err();
        assert!(namespace.message.contains("is a namespace"));

        let field = parse(
            "alkane \"t\" {\n  id = [4, 1]\n}\ncontract \"a\" {\n  package = \"a\"\n  args = [alkane.t.foo]\n}",
        )
        .unwrap_err();
        assert!(field.message.contains("available fields: id, block, tx"));
    }

    #[test]
    fn rejects_undeclared_reference_names() {
        let alkane =
            parse("contract \"a\" {\n  package = \"a\"\n  args = [alkane.usd.id]\n}").unwrap_err();
        assert!(alkane.message.contains("`alkane.usd` is not declared"));

        let contract =
            parse("contract \"a\" {\n  package = \"a\"\n  args = [contract.usd.id]\n}").unwrap_err();
        assert!(contract.message.contains("`contract.usd` is not declared"));

        let cross_hint = parse(
            "alkane \"usd\" {\n  id = [4, 1]\n}\ncontract \"a\" {\n  package = \"a\"\n  args = [contract.usd.id]\n}",
        )
        .unwrap_err();
        assert!(cross_hint.message.contains("did you mean `alkane.usd`?"));

        let call_target = parse(
            "contract \"a\" {\n  package = \"a\"\n}\ncall \"x\" {\n  contract = \"nope\"\n  method = \"m\"\n}",
        )
        .unwrap_err();
        assert!(call_target.message.contains("contract \"nope\" is not declared"));
    }

    #[test]
    fn per_network_ids_resolve_for_the_active_network() {
        let manifest = parse(
            r#"
alkane "usd" {
  id = {
    labcoat = [4, 65012]
    signet  = [2, 190213]
  }
}

contract "series" {
  package = "series"
  args    = [alkane.usd.id]
}
"#,
        )
        .unwrap();
        assert_eq!(manifest.alkanes[0].id_for("labcoat"), Some((4, 65012)));
        assert_eq!(manifest.alkanes[0].id_for("signet"), Some((2, 190213)));
        assert_eq!(manifest.alkanes[0].id_for("mainnet"), None);

        let labcoat = ResolvedIds::from_manifest(&manifest, "labcoat").unwrap();
        assert_eq!(
            eval_scalar(&positional(&manifest.contracts[0].args)[0], &labcoat, 0).unwrap(),
            Some("4:65012".into())
        );
        let signet = ResolvedIds::from_manifest(&manifest, "signet").unwrap();
        assert_eq!(
            eval_scalar(&positional(&manifest.contracts[0].args)[0], &signet, 0).unwrap(),
            Some("2:190213".into())
        );

        let unbound = ResolvedIds::from_manifest(&manifest, "mainnet").unwrap_err();
        assert_eq!(unbound.code, "MANIFEST_INVALID");
        assert!(unbound.message.contains("no id for network `mainnet`"));
        assert!(unbound.message.contains("bound: labcoat, signet"));
    }

    #[test]
    fn rejects_malformed_id_maps() {
        let unknown_network =
            parse("alkane \"t\" {\n  id = { moonnet = [4, 1] }\n}").unwrap_err();
        assert!(unknown_network.message.contains("unknown network 'moonnet'"));

        let empty = parse("alkane \"t\" {\n  id = {}\n}").unwrap_err();
        assert!(empty.message.contains("bind at least one network"));

        let duplicate = parse(
            "alkane \"t\" {\n  id = { regtest = [4, 1], \"regtest\" = [4, 2] }\n}",
        )
        .unwrap_err();
        assert!(duplicate.message.contains("duplicate network `regtest`"));

        let bad_value = parse("alkane \"t\" {\n  id = { regtest = 4 }\n}").unwrap_err();
        assert!(bad_value
            .message
            .contains("array of two non-negative integers"));
    }

    #[test]
    fn rejects_malformed_alkane_blocks() {
        for id in ["4", "[4]", "[4, 5, 6]", "[\"4\", 5]", "\"4:5\""] {
            let err = parse(&format!("alkane \"t\" {{\n  id = {id}\n}}")).unwrap_err();
            assert!(
                err.message.contains("array of two non-negative integers"),
                "id = {id}: {}",
                err.message
            );
        }
        let missing = parse("alkane \"t\" {}").unwrap_err();
        assert!(missing.message.contains("missing required attribute `id`"));

        let unknown = parse("alkane \"t\" {\n  id = [4, 1]\n  reserve = 2\n}").unwrap_err();
        assert!(unknown.message.contains("unknown attribute `reserve`"));

        let duplicate = parse(
            "alkane \"t\" {\n  id = [4, 1]\n}\nalkane \"t\" {\n  id = [4, 2]\n}",
        )
        .unwrap_err();
        assert!(duplicate.message.contains("duplicate alkane"));

        let reserved = parse("alkane \"height\" {\n  id = [4, 1]\n}").unwrap_err();
        assert!(reserved.message.contains("reserved"));
    }

    #[test]
    fn scope_guard_rejects_control_flow() {
        let conditional =
            parse("contract \"a\" {\n  package = \"a\"\n  args = [height > 10 ? 1 : 2]\n}")
                .unwrap_err();
        assert!(conditional.message.contains("conditional"));

        let function =
            parse("contract \"a\" {\n  package = \"a\"\n  args = [max(1, 2)]\n}").unwrap_err();
        assert!(function.message.contains("function calls"));

        let for_expr =
            parse("contract \"a\" {\n  package = \"a\"\n  args = [[for i in [1, 2] : i]]\n}")
                .unwrap_err();
        assert!(for_expr.message.contains("for expressions"));
    }

    #[test]
    fn rejects_unknown_attributes_blocks_and_duplicates() {
        assert!(parse("contract \"a\" {\n  package = \"a\"\n  if = 1\n}")
            .unwrap_err()
            .message
            .contains("unknown attribute `if`"));
        assert!(parse("network \"x\" {}")
            .unwrap_err()
            .message
            .contains("unknown block `network`"));
        assert!(parse(
            "contract \"a\" {\n  package = \"a\"\n}\ncontract \"a\" {\n  package = \"a\"\n}"
        )
        .unwrap_err()
        .message
        .contains("duplicate contract"));
        assert!(parse("contract \"height\" {\n  package = \"a\"\n}")
            .unwrap_err()
            .message
            .contains("reserved"));
    }

    #[test]
    fn rejects_both_and_neither_artifact_sources() {
        assert!(
            parse("contract \"a\" {\n  package = \"a\"\n  wasm = \"a.wasm\"\n}")
                .unwrap_err()
                .message
                .contains("not both")
        );
        assert!(parse("contract \"a\" {\n  reserve = 1\n}")
            .unwrap_err()
            .message
            .contains("Cargo package or a wasm path"));
    }

    #[test]
    fn calls_keep_declaration_order() {
        let manifest = parse(
            r#"
contract "token" {
  package = "token"
}

call "zeta" {
  contract = "token"
  method   = "fund"
}

call "alpha" {
  contract = "token"
  method   = "transfer"
}
"#,
        )
        .unwrap();
        let labels: Vec<_> = manifest.calls.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, ["zeta", "alpha"]);
    }

    #[test]
    fn insert_contract_rejects_oversized_ids() {
        let manifest = parse("contract \"a\" {\n  package = \"a\"\n}").unwrap();
        let mut ids = ResolvedIds::from_manifest(&manifest, "labcoat").unwrap();
        ids.insert_contract("a", "4:65011").unwrap();
        assert_eq!(ids.contract_id("a"), Some("4:65011".into()));
        let err = ids
            .insert_contract("a", "4:340282366920938463463374607431768211455")
            .unwrap_err();
        assert!(err.message.contains("fitting u64"));
    }
}
