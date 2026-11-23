---
title: Declarative deployments
description: Describe a multi-contract topology in alkanes.hcl, review the plan, and apply it repeatably.
---

`labcoat deploy` deploys one contract. When a project has several contracts
that reference each other, describe the whole topology in `alkanes.hcl` and let
`labcoat plan` and `labcoat apply` reconcile it against the chain.

## Plan, then apply

```bash
labcoat plan
labcoat apply --broadcast
```

`labcoat plan` builds every referenced package, resolves references, and
compares the manifest with `labcoat.lock` and chain state. It never loads a
signer or broadcasts. `labcoat apply` without `--broadcast` prints the same
plan; add `--broadcast` to execute the pending actions in dependency order.

Each action in the plan has one status:

| Status | Meaning |
|---|---|
| `pending` | Runs on the next `apply --broadcast`. |
| `complete` | Already deployed or called with the same inputs; skipped. |
| `adopt` | A reserved id already holds matching code with no lockfile record; apply records it without redeploying. |
| `drifted` | Exists but no longer matches the manifest or artifact; skipped with a warning instead of silently redeploying. |
| `interrupted` | A previous apply stopped mid-broadcast; verify that transaction before continuing. |
| `blocked` | Cannot proceed, for example because a reserved id holds different code. |

Applying an unchanged manifest a second time is a no-op. Every new project
includes an `alkanes.hcl` that deploys its Counter contract.

## Write the manifest

```hcl
# An Alkane that exists outside this deployment: a name for a fixed id.
alkane "usd" {
  id = {
    labcoat = [4, 65012]
    signet  = [2, 190213]
  }
}

# A managed contract, deployed and owned by this manifest.
contract "token" {
  package = "my_token"
  reserve = 65011
  args = {
    supply = 100
  }
}

contract "market" {
  package = "my_market"
  args = {
    base   = contract.token.id
    quote  = alkane.usd.id
    expiry = height + 100
  }
}

# Configuration that completes the deployment.
call "seed_market" {
  contract = "market"
  method   = "fund"
  inputs   = ["${contract.token.id}:100"]
}
```

- **`contract`** blocks name a Cargo `package` (or a prebuilt `wasm`), an
  optional `reserve` number that deploys to the predictable id `4:<reserve>`,
  and constructor `args`.
- **`alkane`** blocks bind a name to an id that already exists. The id is
  either one `[block, tx]` pair or a per-network map; `plan` fails when the
  active network has no binding. External Alkanes are never deployed and never
  become dependencies.
- **`call`** blocks invoke a method once, with `args`, `inputs`, `to`,
  `pointer`, `refund`, and `edicts` as needed.

Prefer named `args`: they are matched to the ABI constructor's parameter
names, so the manifest documents each value and key order never matters.
Positional arrays (`args = [1, 100]`) also work.

### References and expressions

`contract.<name>.id` resolves to a managed contract's `"block:tx"` id and
orders that contract before the one referencing it. `alkane.<name>.id`
resolves immediately and adds no ordering. Both also expose `.block` and
`.tx`. Expressions support literals, references, the chain `height` at apply
time, arithmetic, and `"${...}"` string templates.

The manifest is deliberately not a programming language: functions,
conditionals, `for` expressions, and splats are rejected when it is parsed.
Anything conditional belongs in Rust tests.

## Keep usage out of the manifest

A manifest is finished when the topology is deployed and inert. Use `call`
blocks only for configuration that completes the deployment. Protocol usage,
such as minting, trading, or incrementing a counter, belongs in end-to-end
tests or your application.

## End-to-end tests

```bash
labcoat test --e2e
labcoat test --e2e covered_call --no-reset
```

`labcoat test --e2e` resets Labcoat Network, applies `alkanes.hcl`, and runs
the `#[ignore]`d tests in `tests/e2e.rs`. Pass a test-name filter to run a
subset, and `--no-reset` to keep the current chain. E2e runs only target
Labcoat Network and refuse any shared network.

```rust
use labcoat_test::e2e::{Call, E2e};

#[test]
#[ignore = "network e2e — run with `labcoat test --e2e`"]
fn counter_increments_across_wallets() {
    let e2e = E2e::from_env().unwrap();
    let counter = e2e.contract("counter").unwrap();
    assert_eq!(e2e.simulate_uint(&counter, "get_count", &[]).unwrap(), 0);

    // A disposable wallet funded from the faucet.
    let alice = e2e.wallet("alice", 1.0).unwrap();
    e2e.call(Call::new(&counter, "increment").wallet(&alice))
        .unwrap()
        .success()
        .unwrap();
    assert_eq!(e2e.simulate_uint(&counter, "get_count", &[]).unwrap(), 1);
}
```

`e2e.contract` resolves names from the applied manifest, `e2e.wallet` creates
funded wallets for multi-party flows, and `Call` and `Deploy` builders cover
inputs, recipients, pointers, and edicts. The
[covered-call example](https://github.com/jonatns/labcoat/tree/main/docs/examples/covered-call)
shows a complete two-wallet protocol test.

## Public networks

`plan` and `apply` accept the global `--network`, `--signer`, and `--yes`
options. Non-regtest networks require an explicit signing approval, and
`--signer psbt-file:<dir>` hands every transaction to an external signer, as
described in [Atomic exchanges](/docs/exchanges/#sign-with-an-external-wallet).
Mainnet deployment controls are not production-ready; see
[Stability and releases](/docs/reference/stability/).
