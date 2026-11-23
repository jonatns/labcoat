//! Upgradeable counter — a contract behind a standard beacon proxy.
//!
//! Set up a project to run this:
//!
//! ```sh
//! labcoat init upgrade-lab && cd upgrade-lab
//! labcoat new auth-token --template auth-token
//! labcoat new beacon --template upgradeable-beacon
//! labcoat new beacon-proxy --template beacon-proxy
//! ```
//!
//! Copy this file over `deploy.rs` and the sibling `e2e.rs` over
//! `tests/e2e.rs` (its `add` call needs the method the tutorial adds to
//! the counter), then `labcoat plan` and `labcoat apply --broadcast`, or
//! `labcoat test --e2e`.
//!
//! To upgrade, change `contracts/counter` and apply again: the plan
//! deploys a new `counter_logic` and points the beacon at it. `counter`
//! keeps its id and its storage.

use labcoat_test::deploy::{run, Deployment};

fn main() {
    run(|d: &Deployment| {
        // Every standard proxy and beacon clones its auth token from
        // 4:65517. A fresh chain has none, so deploy the template first.
        // Opcode 100 only reads the symbol: the template itself is never
        // initialized.
        d.contract("auth_token", "auth-token")
            .reserve(65517)
            .init(100);

        // The code. Replaceable: changed code deploys a new instance.
        let logic = d.implementation("counter_logic", "counter");

        // The pointer. Deployed once; Labcoat keeps it pointing at the
        // current `counter_logic`.
        let beacon = d.beacon("counter_beacon", "beacon", &logic);

        // The contract everyone talks to. Deployed once; its id and its
        // storage survive every upgrade.
        let counter = d.beacon_proxy("counter", "beacon-proxy", &beacon);

        // Initialize the proxy's storage by calling the logic's
        // initializer through it.
        d.call("init_counter", &counter, "initialize")
            .abi_of(&logic);
    });
}
