//! The contract between the two halves of a deployment: every graph the
//! script API (`labcoat`) can emit must be one the engine
//! (`labcoat_core::deployment`) accepts.

use labcoat::Deployment;
use labcoat_core::deployment::{self, Field, Lifecycle, Source, Step, Value};

fn parse(d: &Deployment) -> deployment::Graph {
    deployment::parse(&d.graph().to_string(), &d.network()).unwrap()
}

#[test]
fn every_builder_emits_a_graph_the_engine_accepts() {
    let d = Deployment::for_network("signet");
    let usd = d
        .external("usd")
        .on("labcoat", 4, 65012)
        .on("signet", 2, 190213);
    let anywhere = d.external("frbtc").id_everywhere(32, 0);
    let token = d
        .contract("token", "my-token")
        .reserve(65011)
        .arg("supply", 100)
        .arg("quote", usd.id());
    d.contract_wasm("prebuilt", "build/prebuilt.wasm")
        .init(100)
        .positional(token.block())
        .positional(token.tx());
    let template = d
        .contract("template", "beacon-proxy")
        .reserve(65520)
        .init(0x8fff);
    let logic = d.implementation("logic", "my-logic");
    let beacon = d.beacon("beacon", "beacon", &logic).auth_units(2);
    let proxy = d.beacon_proxy("live", "beacon-proxy", &beacon);
    let clone = d
        .clone_of("live2", &template)
        .init("initialize")
        .arg("beacon", beacon.id());
    let vault = d.proxy("vault", "upgradeable", &logic).reserve(65530);
    d.call("init", &proxy, "initialize")
        .abi_of(&logic)
        .arg("expiry", d.height() + 100)
        .inputs(token.amount(5))
        .inputs(anywhere.amount(0))
        .to("bcrt1qexample")
        .pointer("v1")
        .refund("v0")
        .edict(edict(&token));
    d.call("poke", &clone, 1)
        .positional(7)
        .positional(d.height() - 1);
    d.setting("fee", &vault, 30)
        .uint()
        .read("get_fee")
        .write("set_fee");

    let graph = parse(&d);
    let names: Vec<&str> = graph.steps.iter().map(Step::name).collect();
    assert_eq!(
        names,
        [
            "usd",
            "frbtc",
            "token",
            "prebuilt",
            "template",
            "logic",
            "beacon",
            "beacon_implementation",
            "live",
            "live2",
            "vault",
            "vault_implementation",
            "init",
            "poke",
            "fee"
        ]
    );

    let contract = |name: &str| match graph.steps.iter().find(|step| step.name() == name) {
        Some(Step::Contract(contract)) => contract.clone(),
        other => panic!("{name} is not a contract: {other:?}"),
    };
    assert_eq!(contract("token").reserve_number().unwrap(), Some(65011));
    assert_eq!(contract("logic").lifecycle, Lifecycle::Replaceable);
    assert_eq!(contract("beacon").lifecycle, Lifecycle::Pinned);
    assert!(matches!(contract("live2").source, Source::Clone { .. }));
    assert_eq!(contract("prebuilt").init.method.as_deref(), Some("100"));
    assert_eq!(contract("template").init.method.as_deref(), Some("36863"));

    // The beacon's setting spends the auth token its deploy created.
    let Some(Step::Setting(setting)) = graph
        .steps
        .iter()
        .find(|step| step.name() == "beacon_implementation")
    else {
        panic!("missing beacon setting");
    };
    assert_eq!(setting.read.as_deref(), Some("implementation"));
    assert_eq!(setting.write.as_deref(), Some("upgrade_to"));
    let mut references = Vec::new();
    setting.tx.inputs[0].references(&mut references);
    assert_eq!(references[0].field, Field::Created);
    assert_eq!(references[0].index, Some(1));

    // The plain proxy exposes no view, so its setting has no read.
    let Some(Step::Setting(setting)) = graph
        .steps
        .iter()
        .find(|step| step.name() == "vault_implementation")
    else {
        panic!("missing proxy setting");
    };
    assert_eq!(setting.read, None);
    assert_eq!(setting.write.as_deref(), Some("upgrade"));
}

/// An edict built from a handle: `block:tx:amount:target`.
fn edict(token: &labcoat::Contract) -> labcoat::Value {
    labcoat::Value::concat([token.id(), labcoat::Value::lit(":5:v0")])
}

#[test]
fn values_keep_their_symbolic_form_until_they_resolve() {
    let d = Deployment::for_network("labcoat");
    let token = d.contract("token", "token");
    d.contract("market", "market")
        .arg("base", token.id())
        .arg("expiry", d.height() + 100);
    let graph = parse(&d);
    let Step::Contract(market) = &graph.steps[1] else {
        panic!("market is a contract");
    };
    let rendered: Vec<String> = market.init.args.values().map(Value::render).collect();
    assert_eq!(rendered, ["<token.id>", "<height+100>"]);
}
