//! Covered-call deployment — the declarative half of what
//! tests/wallets/covered-call.sh used to do imperatively.
//!
//! Deployment only: the script is done when the contracts are deployed and
//! inert. Value-moving operations (the writer escrowing tFIRE, the buyer
//! flow) are protocol usage and live in tests/e2e.rs.
//!
//! Reserve numbers are fixed: `labcoat test --e2e` resets the chain before
//! applying, so every reserve is free again.

use labcoat_deploy::{run, Deployment};

fn main() {
    run(|d: &Deployment| {
        // tUSD is buyer-deployed in the e2e test at reserve 65012 — it
        // exists outside this deployment, so it is an external reference:
        // a name for a fixed id, never deployed here. This fixture only
        // ever runs on Labcoat Network, so one binding covers it; a shared
        // deployment binds each network:
        //   d.external("usd").on("labcoat", 4, 65012).on("signet", 2, 190213)
        let usd = d.external("usd").id_everywhere(4, 65012);

        // Named args are matched to the ABI constructor's parameter names,
        // so the script documents each value instead of relying on
        // position.
        let fire = d
            .contract("fire", "strata_test_token")
            .reserve(65011)
            .arg("variant", 1) // tFIRE
            .arg("supply", 100);

        let writer_claims = d
            .contract("writer_claims", "strata_test_token")
            .reserve(65013)
            .arg("variant", 3) // WRITER
            .arg("supply", 100);

        d.contract("series", "strata_series")
            .reserve(65014)
            .arg("underlying", fire.id())
            .arg("quote", usd.id())
            .arg("writer_claim", writer_claims.id())
            .arg("contract_size", 1)
            .arg("strike", 75)
            // Evaluated once, when the step runs, and recorded.
            .arg("expiry", d.height() + 100)
            .arg("supply", 100);
    });
}
