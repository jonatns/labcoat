//! Deployment scripts: describe a project's deployment in Rust, and let
//! `labcoat plan` / `labcoat deploy` review and execute it.
//!
//! A deployment script is the `deploy.rs` example target of a Labcoat
//! project. It *declares* steps and never executes them: every builder
//! returns a handle, the script prints the resulting graph, and the
//! `labcoat` CLI — the only process that holds a signer — plans and deploys
//! it. A script therefore cannot read a step's result and branch on it;
//! anything that reacts to chain results belongs in `tests/e2e.rs` or the
//! application.
//!
//! ```no_run
//! use labcoat::{run, Deployment};
//!
//! fn main() {
//!     run(|d: &Deployment| {
//!         let usd = d.external("usd").on("labcoat", 4, 65012).on("signet", 2, 190213);
//!         let token = d.contract("token", "my_token").arg("supply", 100);
//!         d.contract("market", "my_market")
//!             .arg("base", token.id())
//!             .arg("quote", usd.id())
//!             .arg("expiry", d.height() + 100);
//!     });
//! }
//! ```
//!
//! Step lifecycles:
//!
//! - [`Deployment::contract`], [`Deployment::clone_of`], and
//!   [`Deployment::call`] are **pinned**: they run once per network, and
//!   editing one after it ran is a conflict that `labcoat deploy` refuses
//!   until `--replace <name>` names it.
//! - [`Deployment::implementation`] is **replaceable**: changed code
//!   deploys a new instance and keeps the old record. Use it only for code
//!   reached through an upgradeable pointer.
//! - [`Deployment::setting`] **converges**: Labcoat reads the current value
//!   and sends the write call only when it differs.

use serde_json::{json, Value as Json};
use std::cell::RefCell;
use std::rc::Rc;

/// The graph schema this module emits and `labcoat` consumes.
pub const GRAPH_SCHEMA: &str = "labcoat/deployment/v1";

/// Standard upgradeable-contract opcodes (alkanes-std-upgradeable,
/// alkanes-std-upgradeable-beacon, alkanes-std-beacon-proxy).
pub const OPCODE_PROXY_INITIALIZE: u128 = 0x7fff;
pub const OPCODE_UPGRADE: u128 = 0x7ffe;
pub const OPCODE_BEACON_IMPLEMENTATION: u128 = 0x7ffd;

/// A value used in step arguments: a literal, a reference to another
/// step's id, the chain height when the step runs, or a concatenation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Value(Json);

impl Value {
    /// A literal, rendered with `ToString`.
    pub fn lit(value: impl ToString) -> Self {
        Value(json!({ "lit": value.to_string() }))
    }

    fn reference(step: &str, field: &str, index: Option<usize>) -> Self {
        let mut reference = json!({ "step": step, "field": field });
        if let Some(index) = index {
            reference["index"] = json!(index);
        }
        Value(json!({ "ref": reference }))
    }

    /// Concatenate values into one string, e.g. an `id:amount` input.
    pub fn concat(parts: impl IntoIterator<Item = Value>) -> Self {
        Value(json!({ "concat": parts.into_iter().map(|part| part.0).collect::<Vec<_>>() }))
    }

    fn offset(self, delta: i64) -> Self {
        let Some(current) = self.0.get("height").and_then(|h| h.get("offset")) else {
            panic!("deploy.rs: arithmetic is only supported on `d.height()`");
        };
        let current = current.as_i64().unwrap_or(0);
        Value(json!({ "height": { "offset": current + delta } }))
    }
}

impl std::ops::Add<i64> for Value {
    type Output = Value;
    fn add(self, rhs: i64) -> Value {
        self.offset(rhs)
    }
}

impl std::ops::Sub<i64> for Value {
    type Output = Value;
    fn sub(self, rhs: i64) -> Value {
        self.offset(-rhs)
    }
}

macro_rules! literal_values {
    ($($ty:ty),*) => {$(
        impl From<$ty> for Value {
            fn from(value: $ty) -> Self {
                Value::lit(value)
            }
        }
    )*};
}
literal_values!(u8, u16, u32, u64, u128, usize, i32, i64, &str, String, &String);

impl From<&Value> for Value {
    fn from(value: &Value) -> Self {
        value.clone()
    }
}

#[derive(Debug)]
struct Inner {
    network: String,
    steps: Vec<Json>,
}

impl Inner {
    fn index_of(&self, name: &str) -> usize {
        self.steps
            .iter()
            .position(|step| step["name"] == name)
            .unwrap_or_else(|| panic!("deploy.rs: step `{name}` was not declared"))
    }
}

type Shared = Rc<RefCell<Inner>>;

fn update(shared: &Shared, name: &str, edit: impl FnOnce(&mut Json)) {
    let mut inner = shared.borrow_mut();
    let index = inner.index_of(name);
    edit(&mut inner.steps[index]);
}

fn push_arg(step: &mut Json, key: &str, name: Option<&str>, value: Value) {
    let args = &mut step[key]["args"];
    match name {
        Some(name) => {
            if args.get("positional").is_some() {
                panic!(
                    "deploy.rs: step `{}` mixes named and positional args",
                    step["name"]
                );
            }
            let named = args
                .as_object_mut()
                .expect("args object")
                .entry("named")
                .or_insert_with(|| json!([]));
            let list = named.as_array_mut().expect("named args");
            list.retain(|pair| pair[0] != name);
            list.push(json!([name, value.0]));
        }
        None => {
            if args.get("named").is_some() {
                panic!(
                    "deploy.rs: step `{}` mixes named and positional args",
                    step["name"]
                );
            }
            args.as_object_mut()
                .expect("args object")
                .entry("positional")
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .expect("positional args")
                .push(value.0);
        }
    }
}

/// The deployment being declared. Obtain one through [`run`].
#[derive(Debug, Clone)]
pub struct Deployment {
    inner: Shared,
}

impl Deployment {
    /// A deployment for `network`, for unit-testing a script's graph.
    pub fn for_network(network: &str) -> Self {
        Self {
            inner: Rc::new(RefCell::new(Inner {
                network: network.to_string(),
                steps: Vec::new(),
            })),
        }
    }

    /// The network this plan targets (`labcoat`, `signet`, …).
    pub fn network(&self) -> String {
        self.inner.borrow().network.clone()
    }

    /// The chain height at the moment a step runs. It is evaluated once,
    /// when the step executes, and recorded; later plans never re-evaluate
    /// it. Supports `+` and `-` with an integer.
    pub fn height(&self) -> Value {
        Value(json!({ "height": { "offset": 0 } }))
    }

    fn add(&self, step: Json) {
        let name = step["name"].as_str().expect("step name").to_string();
        let valid = !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        if !valid {
            panic!("deploy.rs: step name `{name}` must use only letters, digits, `_`, and `-`");
        }
        let mut inner = self.inner.borrow_mut();
        if inner.steps.iter().any(|existing| existing["name"] == name) {
            panic!("deploy.rs: duplicate step name `{name}`");
        }
        inner.steps.push(step);
    }

    /// An Alkane that exists outside this deployment: a name for a fixed
    /// id. It is never deployed. Bind it with [`External::id`] (every
    /// network) or [`External::on`] (one network each).
    pub fn external(&self, name: &str) -> External {
        self.add(json!({ "kind": "external", "name": name, "ids": {} }));
        External {
            shared: self.inner.clone(),
            name: name.to_string(),
        }
    }

    fn add_contract(&self, name: &str, source: Json, lifecycle: &str) -> Contract {
        self.add(json!({
            "kind": "contract",
            "name": name,
            "source": source,
            "lifecycle": lifecycle,
            "init": { "args": {} },
            "tx": {},
        }));
        Contract {
            shared: self.inner.clone(),
            name: name.to_string(),
        }
    }

    /// A contract deployed once from a Cargo contract package. Pinned.
    pub fn contract(&self, name: &str, package: &str) -> Contract {
        self.add_contract(
            name,
            json!({ "type": "package", "package": package }),
            "pinned",
        )
    }

    /// A contract deployed once from a prebuilt raw `.wasm`, relative to
    /// the project root. Pinned.
    pub fn contract_wasm(&self, name: &str, path: &str) -> Contract {
        self.add_contract(name, json!({ "type": "wasm", "wasm": path }), "pinned")
    }

    /// Code reached through an upgradeable pointer (a proxy or beacon).
    /// Replaceable: when the package's code changes, the plan deploys a new
    /// instance and keeps the old record instead of reporting a conflict.
    pub fn implementation(&self, name: &str, package: &str) -> Contract {
        self.add_contract(
            name,
            json!({ "type": "package", "package": package }),
            "replaceable",
        )
    }

    /// A new instance that reuses the code of an already-deployed template
    /// (a factory clone: cellpack target `[5,n]` for a `2:n` template,
    /// `[6,n]` for a `4:n` template). Pinned.
    pub fn clone_of(&self, name: &str, template: &impl Addressable) -> Contract {
        self.add_contract(
            name,
            json!({ "type": "clone", "of": template.id().0 }),
            "pinned",
        )
    }

    /// A call that runs once to complete the deployment. Pinned. `method`
    /// is an ABI method name or a decimal opcode.
    pub fn call(&self, name: &str, contract: &impl Addressable, method: impl ToString) -> Call {
        self.add(json!({
            "kind": "call",
            "name": name,
            "contract": contract.id().0,
            "call": { "method": method.to_string(), "args": {} },
            "tx": {},
        }));
        Call {
            shared: self.inner.clone(),
            name: name.to_string(),
        }
    }

    /// A mutable on-chain value that Labcoat keeps equal to `value`: it
    /// reads the current value and sends the write call only when they
    /// differ. Configure it with [`Setting::read`] and [`Setting::write`].
    pub fn setting(
        &self,
        name: &str,
        contract: &impl Addressable,
        value: impl Into<Value>,
    ) -> Setting {
        self.add(json!({
            "kind": "setting",
            "name": name,
            "contract": contract.id().0,
            "value": value.into().0,
            "valueType": "alkane-id",
            "tx": {},
        }));
        Setting {
            shared: self.inner.clone(),
            name: name.to_string(),
        }
    }

    /// A standard upgradeable beacon (`alkanes-std-upgradeable-beacon`)
    /// from `package`, pointing at `implementation`. Declares the beacon
    /// contract and a `<name>_implementation` setting that upgrades the
    /// beacon whenever `implementation` is replaced. Initialization mints
    /// one auth token to the deploying wallet; upgrades spend and return it.
    pub fn beacon(&self, name: &str, package: &str, implementation: &Contract) -> Upgradeable {
        self.upgradeable(
            name,
            package,
            implementation,
            Some("implementation"),
            "upgrade_to",
        )
    }

    /// A standard upgradeable proxy (`alkanes-std-upgradeable`) from
    /// `package`, delegating to `implementation`. Declares the proxy
    /// contract and a `<name>_implementation` setting. The proxy exposes no
    /// view of its pointer, so the setting compares against the value
    /// Labcoat last recorded rather than a chain read.
    pub fn proxy(&self, name: &str, package: &str, implementation: &Contract) -> Upgradeable {
        self.upgradeable(name, package, implementation, None, "upgrade")
    }

    fn upgradeable(
        &self,
        name: &str,
        package: &str,
        implementation: &Contract,
        read: Option<&str>,
        write: &str,
    ) -> Upgradeable {
        let contract = self
            .contract(name, package)
            .init("initialize")
            .arg("implementation", implementation.id())
            .arg("auth_token_units", 1);
        let auth_token = contract.created(1);
        let setting = self
            .setting(
                &format!("{name}_implementation"),
                &contract,
                implementation.id(),
            )
            .write(write)
            .inputs(Value::concat([auth_token.clone(), Value::lit(":1")]));
        if let Some(read) = read {
            setting.read(read);
        } else {
            update(&setting.shared, &setting.name, |step| {
                step["initializerArg"] = json!("implementation");
            });
        }
        Upgradeable {
            contract,
            auth_token,
        }
    }

    /// A standard beacon proxy (`alkanes-std-beacon-proxy`) from `package`
    /// that follows `beacon`. Pinned: it is created once and never changes.
    pub fn beacon_proxy(&self, name: &str, package: &str, beacon: &Upgradeable) -> Contract {
        self.contract(name, package)
            .init("initialize")
            .arg("beacon", beacon.id())
    }

    /// The declared graph as JSON.
    pub fn graph(&self) -> Json {
        let inner = self.inner.borrow();
        json!({
            "schema": GRAPH_SCHEMA,
            "network": inner.network,
            "steps": inner.steps,
        })
    }
}

/// Anything with an Alkane id a step can target or clone.
pub trait Addressable {
    /// The `block:tx` id.
    fn id(&self) -> Value;
}

macro_rules! id_fields {
    ($ty:ty) => {
        impl Addressable for $ty {
            fn id(&self) -> Value {
                Value::reference(&self.name, "id", None)
            }
        }

        impl $ty {
            /// The `block:tx` id.
            pub fn id(&self) -> Value {
                Value::reference(&self.name, "id", None)
            }

            /// The id's block number.
            pub fn block(&self) -> Value {
                Value::reference(&self.name, "block", None)
            }

            /// The id's tx number.
            pub fn tx(&self) -> Value {
                Value::reference(&self.name, "tx", None)
            }

            /// `id:amount`, the form transaction inputs take.
            pub fn amount(&self, amount: u128) -> Value {
                Value::concat([self.id(), Value::lit(format!(":{amount}"))])
            }
        }
    };
}

/// Handle to an [`external`](Deployment::external) Alkane.
#[derive(Debug, Clone)]
pub struct External {
    shared: Shared,
    name: String,
}
id_fields!(External);

impl External {
    /// Bind the same id on every network.
    pub fn id_everywhere(self, block: u64, tx: u64) -> Self {
        self.on("*", block, tx)
    }

    /// Bind the id on one network. Planning fails on a network with no
    /// binding.
    pub fn on(self, network: &str, block: u64, tx: u64) -> Self {
        update(&self.shared, &self.name, |step| {
            step["ids"][network] = json!([block, tx]);
        });
        self
    }
}

macro_rules! tx_shape {
    ($ty:ty) => {
        impl $ty {
            /// Extra transaction input: an alkanes `block:tx:amount` (see
            /// `amount()` on handles) or bitcoin `B:sats`. Repeatable.
            pub fn inputs(self, input: impl Into<Value>) -> Self {
                let input = input.into();
                update(&self.shared, &self.name, |step| {
                    let tx = step["tx"].as_object_mut().expect("tx object");
                    tx.entry("inputs")
                        .or_insert_with(|| json!([]))
                        .as_array_mut()
                        .expect("inputs")
                        .push(input.0);
                });
                self
            }

            /// Recipient address of the protostone outputs (defaults to the
            /// signing wallet's primary address).
            pub fn to(self, address: impl Into<Value>) -> Self {
                let address = address.into();
                update(&self.shared, &self.name, |step| {
                    step["tx"]["to"] = address.0
                });
                self
            }

            /// Pointer target `vN`/`pN` (default v0).
            pub fn pointer(self, target: &str) -> Self {
                update(&self.shared, &self.name, |step| {
                    step["tx"]["pointer"] = json!(target)
                });
                self
            }

            /// Refund target (defaults to the pointer target).
            pub fn refund(self, target: &str) -> Self {
                update(&self.shared, &self.name, |step| {
                    step["tx"]["refund"] = json!(target)
                });
                self
            }

            /// Edict `block:tx:amount:target` appended to the protostone.
            pub fn edict(self, edict: impl Into<Value>) -> Self {
                let edict = edict.into();
                update(&self.shared, &self.name, |step| {
                    let tx = step["tx"].as_object_mut().expect("tx object");
                    tx.entry("edicts")
                        .or_insert_with(|| json!([]))
                        .as_array_mut()
                        .expect("edicts")
                        .push(edict.0);
                });
                self
            }
        }
    };
}

/// Handle to a contract step.
#[derive(Debug, Clone)]
pub struct Contract {
    shared: Shared,
    name: String,
}
id_fields!(Contract);
tx_shape!(Contract);

impl Contract {
    /// Deploy to reserved number N: the predictable id `4:N` (cellpack
    /// target `[3,N]`).
    pub fn reserve(self, number: u128) -> Self {
        update(&self.shared, &self.name, |step| {
            step["reserve"] = json!(number.to_string());
        });
        self
    }

    /// The initializer the deploy transaction calls: an ABI method name or
    /// a decimal opcode. Defaults to the ABI's opcode-0 constructor.
    pub fn init(self, method: impl ToString) -> Self {
        update(&self.shared, &self.name, |step| {
            step["init"]["method"] = json!(method.to_string());
        });
        self
    }

    /// A named initializer argument, matched to the ABI parameter name.
    pub fn arg(self, name: &str, value: impl Into<Value>) -> Self {
        let value = value.into();
        update(&self.shared, &self.name, |step| {
            push_arg(step, "init", Some(name), value)
        });
        self
    }

    /// A positional initializer argument, in ABI parameter order (or a raw
    /// cellpack value for a numeric-opcode initializer).
    pub fn positional(self, value: impl Into<Value>) -> Self {
        let value = value.into();
        update(&self.shared, &self.name, |step| {
            push_arg(step, "init", None, value)
        });
        self
    }

    /// The id of the `index`-th Alkane the deploy transaction created.
    /// Index 0 is the contract itself; index 1 is, for standard
    /// upgradeable contracts, the auth token minted at initialization.
    pub fn created(&self, index: usize) -> Value {
        Value::reference(&self.name, "created", Some(index))
    }
}

/// Handle to a standard beacon or proxy declared by
/// [`Deployment::beacon`] or [`Deployment::proxy`].
#[derive(Debug, Clone)]
pub struct Upgradeable {
    contract: Contract,
    auth_token: Value,
}

impl Addressable for Upgradeable {
    fn id(&self) -> Value {
        self.contract.id()
    }
}

impl Upgradeable {
    /// The beacon's or proxy's `block:tx` id. It never changes across
    /// upgrades.
    pub fn id(&self) -> Value {
        self.contract.id()
    }

    /// The underlying contract step.
    pub fn contract(&self) -> &Contract {
        &self.contract
    }

    /// The auth token minted at initialization; whoever holds it may
    /// upgrade.
    pub fn auth_token(&self) -> Value {
        self.auth_token.clone()
    }

    /// Deploy to reserved number N (`4:N`).
    pub fn reserve(self, number: u128) -> Self {
        Self {
            contract: self.contract.reserve(number),
            auth_token: self.auth_token,
        }
    }

    /// Mint `units` auth tokens at initialization instead of one.
    pub fn auth_units(self, units: u128) -> Self {
        Self {
            contract: self.contract.arg("auth_token_units", units),
            auth_token: self.auth_token,
        }
    }
}

/// Handle to a call step.
#[derive(Debug, Clone)]
pub struct Call {
    shared: Shared,
    name: String,
}
tx_shape!(Call);

impl Call {
    /// A named argument, matched to the ABI parameter name.
    pub fn arg(self, name: &str, value: impl Into<Value>) -> Self {
        let value = value.into();
        update(&self.shared, &self.name, |step| {
            push_arg(step, "call", Some(name), value)
        });
        self
    }

    /// A positional argument (a raw cellpack value for a numeric opcode).
    pub fn positional(self, value: impl Into<Value>) -> Self {
        let value = value.into();
        update(&self.shared, &self.name, |step| {
            push_arg(step, "call", None, value)
        });
        self
    }

    /// Resolve the method name against another step's ABI. A call through
    /// a proxy needs this: the proxy's own ABI lists only proxy methods, so
    /// name the implementation whose method this is.
    pub fn abi_of(self, contract: &Contract) -> Self {
        let step_name = contract.name.clone();
        update(&self.shared, &self.name, |step| {
            step["call"]["abi"] = json!(step_name)
        });
        self
    }
}

/// Handle to a setting step.
#[derive(Debug, Clone)]
pub struct Setting {
    shared: Shared,
    name: String,
}
tx_shape!(Setting);

impl Setting {
    /// The view that returns the current value: an ABI method name or a
    /// decimal opcode. Without one, Labcoat compares against the value it
    /// last recorded.
    pub fn read(self, method: impl ToString) -> Self {
        update(&self.shared, &self.name, |step| {
            step["read"] = json!(method.to_string())
        });
        self
    }

    /// The method that sets the value, taking it as its only argument: an
    /// ABI method name or a decimal opcode.
    pub fn write(self, method: impl ToString) -> Self {
        update(&self.shared, &self.name, |step| {
            step["write"] = json!(method.to_string())
        });
        self
    }

    /// Treat the value as a `u128` instead of an Alkane id.
    pub fn uint(self) -> Self {
        update(&self.shared, &self.name, |step| {
            step["valueType"] = json!("u128")
        });
        self
    }
}

/// Build the deployment and hand its graph to `labcoat`. Call this from
/// the `main` of the project's `deploy.rs`.
///
/// `labcoat plan` and `labcoat deploy` run the script with
/// `LABCOAT_DEPLOY_NETWORK` and `LABCOAT_DEPLOY_GRAPH` set and read the
/// graph from that file. Run directly (`cargo run --example deploy`), the
/// script prints the graph for the `labcoat` network to stdout.
pub fn run(build: impl FnOnce(&Deployment)) {
    let network = std::env::var("LABCOAT_DEPLOY_NETWORK").unwrap_or_else(|_| "labcoat".into());
    let deployment = Deployment::for_network(&network);
    build(&deployment);
    let graph = serde_json::to_string_pretty(&deployment.graph()).expect("serializable graph");
    match std::env::var_os("LABCOAT_DEPLOY_GRAPH") {
        Some(path) => std::fs::write(&path, graph).unwrap_or_else(|error| {
            panic!(
                "deploy.rs: cannot write the graph to {}: {error}",
                std::path::Path::new(&path).display()
            )
        }),
        None => println!("{graph}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declares_references_heights_and_named_args() {
        let d = Deployment::for_network("labcoat");
        let usd = d
            .external("usd")
            .on("labcoat", 4, 65012)
            .on("signet", 2, 190213);
        let fire = d
            .contract("fire", "token")
            .reserve(65011)
            .arg("supply", 100);
        d.contract("series", "series")
            .arg("underlying", fire.id())
            .arg("quote", usd.id())
            .arg("expiry", d.height() + 100);
        d.call("fund", &fire, "fund").inputs(fire.amount(100));

        let graph = d.graph();
        assert_eq!(graph["schema"], GRAPH_SCHEMA);
        assert_eq!(graph["network"], "labcoat");
        let steps = graph["steps"].as_array().unwrap();
        assert_eq!(steps.len(), 4);
        assert_eq!(steps[0]["ids"]["signet"], json!([2, 190213]));
        assert_eq!(steps[1]["reserve"], "65011");
        assert_eq!(steps[1]["lifecycle"], "pinned");
        assert_eq!(
            steps[2]["init"]["args"]["named"][2],
            json!(["expiry", { "height": { "offset": 100 } }])
        );
        assert_eq!(
            steps[3]["tx"]["inputs"][0],
            json!({ "concat": [{ "ref": { "step": "fire", "field": "id" } }, { "lit": ":100" }] })
        );
    }

    #[test]
    fn a_repeated_named_arg_replaces_the_earlier_value() {
        let d = Deployment::for_network("labcoat");
        d.contract("token", "token")
            .arg("supply", 1)
            .arg("supply", 2);
        let named = d.graph()["steps"][0]["init"]["args"]["named"].clone();
        assert_eq!(named, json!([["supply", { "lit": "2" }]]));
    }

    #[test]
    fn beacon_declares_a_contract_and_a_converging_setting() {
        let d = Deployment::for_network("labcoat");
        let logic = d.implementation("token_logic", "token");
        let beacon = d.beacon("token_beacon", "upgradeable_beacon", &logic);
        d.beacon_proxy("token", "beacon_proxy", &beacon);

        let graph = d.graph();
        let steps = graph["steps"].as_array().unwrap();
        assert_eq!(steps[0]["lifecycle"], "replaceable");
        assert_eq!(steps[1]["init"]["method"], "initialize");
        assert_eq!(steps[2]["kind"], "setting");
        assert_eq!(steps[2]["name"], "token_beacon_implementation");
        assert_eq!(steps[2]["read"], "implementation");
        assert_eq!(steps[2]["write"], "upgrade_to");
        assert_eq!(
            steps[2]["tx"]["inputs"][0]["concat"][0],
            json!({ "ref": { "step": "token_beacon", "field": "created", "index": 1 } })
        );
        assert_eq!(steps[3]["init"]["args"]["named"][0][0], "beacon");
    }

    #[test]
    #[should_panic(expected = "duplicate step name `token`")]
    fn rejects_duplicate_step_names() {
        let d = Deployment::for_network("labcoat");
        d.contract("token", "token");
        d.contract("token", "token");
    }

    #[test]
    #[should_panic(expected = "arithmetic is only supported on `d.height()`")]
    fn rejects_arithmetic_on_references() {
        let d = Deployment::for_network("labcoat");
        let token = d.contract("token", "token");
        let _ = token.id() + 1;
    }
}
