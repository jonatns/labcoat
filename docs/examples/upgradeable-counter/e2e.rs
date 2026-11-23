//! End-to-end tests against the deployment `deploy.rs` applied on Labcoat
//! Network. Run with `labcoat test --e2e` — plain `cargo test` skips these
//! because they are `#[ignore]`d.

use labcoat_test::e2e::{Call, E2e};

#[test]
#[ignore = "network e2e — run with `labcoat test --e2e`"]
fn counter_works_through_its_proxy() {
    let e2e = E2e::from_env().unwrap();
    // The proxy is the contract everyone talks to; the logic's ABI names
    // the methods it delegates.
    let counter = e2e.contract("counter").unwrap();
    let logic = e2e.contract("counter_logic").unwrap();

    assert_eq!(
        e2e.simulate_uint_via(&counter, &logic, "get_count", &[]).unwrap(),
        0
    );
    e2e.call(Call::new(&counter, "increment").abi(&logic))
        .unwrap()
        .success()
        .unwrap();
    e2e.call(Call::new(&counter, "add").arg(41).abi(&logic))
        .unwrap()
        .success()
        .unwrap();
    assert_eq!(
        e2e.simulate_uint_via(&counter, &logic, "get_count", &[]).unwrap(),
        42
    );

    // Initialization minted one auth token to the deploying wallet;
    // whoever holds it may upgrade the beacon.
    let auth_token = e2e.created("counter_beacon", 1).unwrap();
    let owner = e2e.project_address().unwrap();
    assert_eq!(e2e.balance(&owner, &auth_token).unwrap(), 1);
}
