//! Two-wallet covered-call e2e test — the Rust replacement for the Bash
//! `covered-call.sh` flow. The deployment topology lives in `alkanes.hcl`;
//! every value-moving step — the writer funding the series, the two-party
//! buyer flow — is protocol usage and is driven here.
//!
//! Run with `labcoat test --e2e`. Copy into `tests/e2e.rs` of the Strata
//! project alongside the sibling `alkanes.hcl`.

use labcoat_test::e2e::{Call, Deploy, E2e};

const STRIKE_TOTAL: u128 = 7_500; // 75 tUSD × 100 contracts

#[test]
#[ignore = "network e2e — run with `labcoat test --e2e`"]
fn covered_call_exercise_and_redeem() {
    let e2e = E2e::from_env().unwrap();

    // Manifest-applied topology (writer = project wallet).
    let fire = e2e.contract("fire").unwrap();
    let writer_claims = e2e.contract("writer_claims").unwrap();
    let series = e2e.contract("series").unwrap();

    // The writer escrows 100 tFIRE, which mints the 100 CALL tokens.
    e2e.call(Call::new(&series, "fund").inputs(&format!("{fire}:100")))
        .unwrap()
        .success()
        .unwrap();

    // The buyer is a disposable wallet holding the tUSD supply, matching
    // the Bash flow where the buyer deployed tUSD.
    let buyer = e2e.wallet("buyer", 2.0).unwrap();
    let usd = e2e
        .deploy(
            Deploy::wasm("build/strata_test_token.wasm", "usd")
                .reserve(65012)
                .arg(2)      // variant tUSD
                .arg(10_000) // supply
                .wallet(&buyer),
        )
        .unwrap()
        .success()
        .unwrap()
        .alkanes_id()
        .unwrap();

    // Writer sends 100 CALL tokens to the buyer (edict routes them to the
    // buyer-owned output v1, mirroring `:v1:v1:[series:100:v0]`).
    e2e.call(
        Call::new(&series, "101")
            .inputs(&format!("{series}:100"))
            .to(&buyer.address)
            .pointer("v1")
            .edict(&format!("{series}:100:v0")),
    )
    .unwrap()
    .success()
    .unwrap();

    // Buyer exercises all 100 CALLs for 7,500 tUSD.
    e2e.call(
        Call::new(&series, "10")
            .inputs(&format!("{series}:100,{usd}:{STRIKE_TOTAL}"))
            .wallet(&buyer),
    )
    .unwrap()
    .success()
    .unwrap();

    // Mine to expiry, then the writer redeems the WRITER claims.
    let expiry = e2e.height().unwrap() + 100; // matches height + 100 at apply
    e2e.mine_until(expiry).unwrap();
    e2e.call(
        Call::new(&series, "20").inputs(&format!("{writer_claims}:100")),
    )
    .unwrap()
    .success()
    .unwrap();

    // Same assertions as the Bash script.
    let writer_address = e2e.project_address().unwrap();
    assert_eq!(e2e.balance(&buyer.address, &fire).unwrap(), 100);
    assert_eq!(e2e.balance(&buyer.address, &usd).unwrap(), 2_500);
    assert_eq!(e2e.balance(&buyer.address, &series).unwrap(), 0);
    assert_eq!(e2e.balance(&writer_address, &usd).unwrap(), STRIKE_TOTAL);
    assert_eq!(e2e.balance(&writer_address, &writer_claims).unwrap(), 0);
}
