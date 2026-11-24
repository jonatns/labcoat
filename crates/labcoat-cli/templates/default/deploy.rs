//! This project's deployment, reviewed with `labcoat plan` and executed
//! with `labcoat apply --broadcast`.
//!
//! The script declares steps; it never executes them. Labcoat runs it to
//! read the steps, shows the plan, and only then signs anything, so the
//! script cannot read a step's result and react to it — state-changing
//! usage (like incrementing this counter) belongs in tests/e2e.rs or your
//! application.
//!
//! A step runs once per network. Editing a contract or call after it ran
//! is a conflict that apply refuses until you pass `--replace <name>`.
//!
//!   d.contract(name, package)        deploy a contract package once
//!       .reserve(n)                  at the predictable id 4:n
//!       .arg("supply", 100)          named constructor args (ABI names)
//!   d.external(name).on(network, block, tx)
//!                                    name an Alkane that already exists
//!   d.call(name, &contract, method)  run a configuration call once
//!   other.id(), d.height() + 100     reference another step, or the height
//!
//! Upgradeable contracts: d.implementation, d.beacon, d.beacon_proxy, and
//! d.proxy. See `labcoat docs --llm` for the full API.

use labcoat::{run, Deployment};

fn main() {
    run(|d: &Deployment| {
        d.contract("counter", "counter");
    });
}
