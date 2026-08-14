//! End-to-end tests against the contracts `labcoat apply` deployed on
//! Labcoat Network. Run with `labcoat test --e2e` — plain `cargo test`
//! skips these because they are `#[ignore]`d.

use labcoat_test::e2e::{Call, E2e};

#[test]
#[ignore = "network e2e — run with `labcoat test --e2e`"]
fn counter_increments_across_wallets() {
    let e2e = E2e::from_env().unwrap();
    let counter = e2e.contract("counter").unwrap();

    // The manifest deploys an inert counter; usage starts here.
    assert_eq!(e2e.simulate_uint(&counter, "get_count", &[]).unwrap(), 0);

    // The project wallet increments once.
    e2e.call(Call::new(&counter, "increment"))
        .unwrap()
        .success()
        .unwrap();
    assert_eq!(e2e.simulate_uint(&counter, "get_count", &[]).unwrap(), 1);

    // A disposable, faucet-funded wallet increments again.
    let alice = e2e.wallet("alice", 1.0).unwrap();
    e2e.call(Call::new(&counter, "increment").wallet(&alice))
        .unwrap()
        .success()
        .unwrap();
    assert_eq!(e2e.simulate_uint(&counter, "get_count", &[]).unwrap(), 2);
}
