---
title: Deployment scripts
description: Declare a project's deployment in deploy.rs, review the plan, and deploy each step once per network.
---

A project declares its deployment in `deploy.rs`: the contracts it deploys,
the existing Alkanes they point at, the calls that configure them, and the
contracts it upgrades. `labcoat plan` shows what deploying would do, and
`labcoat deploy` does it, running each step once per network.

## Plan, then deploy

```bash
labcoat plan
labcoat deploy
```

`labcoat plan` builds every referenced package, runs `deploy.rs` to read the
steps it declares, and compares them with the network's
[run file](#the-run-file) and the chain. It never loads a signer or
broadcasts. `labcoat deploy` executes the pending steps in the
order the script declares them.

How strict deploy is depends on the network:

| Network | A step you edited | Signing |
|---|---|---|
| Labcoat Network | Redeploys, with whatever references it | No prompt |
| Custom regtest | Conflict until you pass `--replace` | No prompt |
| Signet, testnet, mainnet | Conflict until you pass `--replace` | Prompt for each transaction, or `--yes` |

`labcoat deploy --dry-run` prints the plan instead, like `labcoat plan`.

Each step in the plan has one status:

| Status | Meaning |
|---|---|
| `pending` | Runs on the next `deploy`. The plan says what it will do: `create`, `clone`, `replace`, `call`, `rerun`, `update`, or `check`. |
| `complete` | Already ran with this definition; skipped. |
| `conflict` | Ran with a different definition, or references a contract that was since replaced. Deploy refuses until you resolve it. See [Steps run once](#steps-run-once). |
| `adopt` | A reserved id already holds matching code with no record; deploy records it without redeploying. |
| `interrupted` | A transaction is awaiting an execution trace, or the previous deploy stopped before it could prepare the reveal. |
| `blocked` | Cannot proceed, for example because a reserved id holds different code, or a step deployed on its own references one that has not run. |

Deploying an unchanged script a second time is a no-op. Every new project
includes a `deploy.rs` that deploys its Counter contract.

### Deploy one step

```bash
labcoat deploy token
```

Naming a step runs that step alone and leaves the other pending steps for
later. Everything it references has to be deployed already; if something is
not, the step is `blocked`, and `--dry-run` names what to deploy first.
`--replace` is checked against the whole deployment, so it takes no step
name.

### Deploy an artifact outside the script

```bash
labcoat deploy --wasm build/token.wasm --name beta --args 5000
```

`--wasm` deploys a prebuilt `.wasm` that `deploy.rs` does not declare: an
experiment, or an extra instance signed by another wallet. Its name,
constructor arguments, reserve number, and transaction options are flags. The
result is recorded in `labcoat.lock` only, so the plan does not know about
it, and Labcoat refuses a name that a `deploy.rs` step already ran under.
Anything that belongs to the project is a step instead:
`d.contract_wasm(name, path)` deploys a prebuilt artifact from the script.

## Write the script

`deploy.rs` uses the `labcoat` Rust library from the `labcoat-deploy` Cargo package.

```rust
use labcoat::{run, Deployment};

fn main() {
    run(|d: &Deployment| {
        // An Alkane that exists outside this deployment: a name for a fixed id.
        let usd = d.external("usd").on("labcoat", 4, 65012).on("signet", 2, 190213);

        // A contract deployed once from a Cargo contract package.
        let token = d
            .contract("token", "my-token")
            .reserve(65011)
            .arg("supply", 100);

        let market = d
            .contract("market", "my-market")
            .arg("base", token.id())
            .arg("quote", usd.id())
            .arg("expiry", d.height() + 100);

        // Configuration that completes the deployment.
        d.call("seed_market", &market, "fund").inputs(token.amount(100));
    });
}
```

The script **declares** steps and never executes them. Each builder returns a
handle; `token.id()` is a reference Labcoat resolves when the step that needs
it runs. Labcoat runs the script to read the steps, shows the plan, and only
then signs anything. A script therefore cannot read a step's result and react
to it: anything that depends on what the chain returns belongs in
[end-to-end tests](#end-to-end-tests) or your application.

Steps run in the order they are declared, and a step can reference only steps
declared before it, so there is no dependency order to work out.

The script is the project's `deploy` Cargo example target. `labcoat init`
registers it; an existing project adds this to its root `Cargo.toml`:

```toml
[[example]]
name = "deploy"
path = "deploy.rs"
```

### Steps

| Builder | Declares |
|---|---|
| `d.contract(name, package)` | A contract deployed once from a Cargo contract package. |
| `d.contract_wasm(name, path)` | A contract deployed once from a prebuilt raw `.wasm`. |
| `d.clone_of(name, &template)` | A new instance that reuses the code of an already-deployed `2:n` or `4:n` template, with no Wasm in the transaction. |
| `d.external(name)` | An Alkane that already exists. Never deployed. |
| `d.call(name, &contract, method)` | A call that runs once to complete the deployment. |
| `d.implementation(name, package)` | Code reached through an upgradeable pointer. See [Upgradeable contracts](#upgradeable-contracts). |
| `d.beacon`, `d.beacon_proxy`, `d.proxy` | The standard upgradeable contracts. |
| `d.setting(name, &contract, value)` | A mutable on-chain value Labcoat keeps equal to `value`. |

Step names are unique across the script. A contract step's name is also the
name `labcoat call`, `labcoat simulate`, and `e2e.contract` resolve.

### Contracts

```rust
d.contract("token", "my-token")
    .reserve(65011)            // deploy to the predictable id 4:65011
    .init("initialize")        // the initializer: an ABI method name or opcode
    .arg("supply", 100);       // named args, matched to ABI parameter names
```

- **`reserve(n)`** deploys to the predictable id `4:n`, which later steps can
  reference before the deploy runs. Without it the contract gets the next free
  id `2:n`, known after the deploy.
- **`init(method)`** names the initializer the deploy transaction calls.
  Without it, Labcoat uses the ABI's opcode-0 constructor.
- **`arg(name, value)`** passes a named argument; `positional(value)` passes
  one by position. A numeric-opcode initializer takes positional raw cellpack
  values.

### Values and references

| Expression | Value when the step runs |
|---|---|
| `100`, `"text"` | A literal. |
| `step.id()` | The step's `block:tx` id. |
| `step.block()`, `step.tx()` | One half of the id, for raw cellpack arguments. |
| `step.amount(n)` | `block:tx:n`, the form transaction inputs take. |
| `contract.created(i)` | The `i`-th Alkane the contract's deploy transaction created. Index 0 is the contract itself. |
| `d.height() + 100` | The chain height plus an offset. |

`d.height()` is evaluated once, when its step runs, and the result is
recorded. A later plan never re-evaluates it, so mining a block does not
change a step that already ran.

`d.network()` returns the network the script is running for, for the rare
value that has to differ per network.

### Calls

```rust
d.call("seed_market", &market, "fund")
    .arg("amount", 100)
    .inputs(token.amount(100))
    .pointer("v1");
```

A call takes the same transaction options as `labcoat call`: `inputs`, `to`,
`pointer`, `refund`, and `edict`. Its method is an ABI method name or a
decimal opcode. Contract steps take the same options for their deploy
transaction.

## Steps run once

Alkanes deployments cannot be changed or deleted. So a contract, clone, or
call is **pinned**: it runs once per network, and Labcoat records its
definition — the code, target, initializer, and arguments. A plan compares the
script with that record. It does not try to make the chain match a script you
edited afterwards.

**On Labcoat Network**, where the chain is local and disposable, `labcoat
deploy` redeploys an edited contract, redeploys whatever references it, and
runs the affected calls again. Each new instance has a new id and empty
storage. The plan shows these steps as pending before you deploy. An edited
contract with a reserved id cannot redeploy, because the id is taken: give it
another number or run `labcoat reset`.

**On every other network**, editing a pinned step after it ran makes the plan
report a `conflict`, and `labcoat deploy` stops with `DEPLOYMENT_CONFLICT` before sending anything:

```text
! Plan for deploy.rs — 0 pending, 1 in conflict

Kind      Name            Status    Id/Target
contract  counter_beacon  conflict  2:2

counter_beacon: its definition changed since it ran — restore it, or pass
`--replace counter_beacon` to deploy a new instance
```

You have two ways out:

- **Restore the step.** Undo the edit and the plan is clean again.
- **Replace it.** `--replace <name>` deploys a new instance with a new id and
  keeps the old one in the record's history:

  ```bash
  labcoat plan --replace token
  labcoat deploy --replace token
  ```

  For a call, `--replace` runs it again.

Replacing a contract changes its id, so every step that referenced the old
instance now disagrees with what it recorded. The plan lists each one as a
conflict until you name it too:

```text
token:  replaces 2:5, which stays on chain and in the record's history
market: references `token` (2:5), which is being replaced — restore it, or
        pass `--replace market` to deploy a new instance
```

That list is the full cost of the replacement: every contract that has to be
redeployed with it. Balances and storage stay with the old instances.

A reserved id can only be used once. To replace a contract that took a
reserve, give the step a new reserve number.

## Existing Alkanes

**An Alkane you do not deploy** is an `external`. Bind its id per network, or
once for every network:

```rust
let usd = d.external("usd").on("labcoat", 4, 65012).on("signet", 2, 190213);
let frbtc = d.external("frbtc").id_everywhere(32, 0);
```

Planning fails on a network with no binding. An external is never deployed
and never needs `--replace`; if you change its id, the steps that consumed the
old one become conflicts.

**A contract you already deployed** — before the script existed, or with
another tool — is recorded with `labcoat import`:

```bash
labcoat import market 2:118
```

Import compares the deployed contract's ABI with the step's local build,
records the id as that step's instance, and broadcasts nothing. For a proxy or
beacon, include the reveal transaction ID so Labcoat can recover its auth
token. Labcoat also looks for that transaction ID in a previous attempt or
in `labcoat.lock`.

```bash
labcoat import counter_beacon 2:2 --txid <reveal-txid>
```

You can reimport the same ID with its reveal transaction to recover missing
auth-token IDs from an earlier import.

**A reserved id that already holds matching code** needs no command: the plan
shows `adopt`, and deploy records it without redeploying. This is how a
standard contract that already sits at a well-known reserve, such as the auth
token at `4:65517`, joins your deployment. A reserve that holds code with a
different ABI is `blocked` instead.

## Upgradeable contracts

A contract's code cannot change, but a contract can delegate to code that is
replaced. Labcoat supports the three standard Alkanes contracts for this:

| Contract | Role |
|---|---|
| Upgradeable proxy | Holds storage and balances, delegates every call to one implementation it points at. |
| Upgradeable beacon | Holds one implementation id for any number of beacon proxies. |
| Beacon proxy | Holds storage and balances, delegates to whatever its beacon points at. |

Vendor them into the project as ordinary contract packages:

```bash
labcoat new auth-token --template auth-token
labcoat new beacon --template upgradeable-beacon
labcoat new beacon-proxy --template beacon-proxy
labcoat new proxy --template upgradeable
```

Then declare the deployment:

```rust
// Every standard proxy and beacon clones its auth token from 4:65517.
d.contract("auth_token", "auth-token").reserve(65517).init(100);

let logic = d.implementation("counter_logic", "counter");
let beacon = d.beacon("counter_beacon", "beacon", &logic);
let counter = d.beacon_proxy("counter", "beacon-proxy", &beacon);
d.call("init_counter", &counter, "initialize").abi_of(&logic);
```

- **`d.implementation`** is the one step that is *replaceable*: when the
  package's code changes, the plan deploys a new instance and keeps the old
  record. No `--replace` is needed, and nothing that references it becomes a
  conflict. Use it only for code reached through a proxy or beacon.
- **`d.beacon`** declares the beacon and a `<name>_implementation` setting.
  The beacon is pinned; the setting keeps its pointer equal to the current
  implementation.
- **`d.beacon_proxy`** is pinned. Its id and storage survive every upgrade.
- **`d.proxy`** declares a standalone upgradeable proxy and its
  `<name>_implementation` setting, for a contract that does not share a
  beacon.
- **`abi_of(&logic)`** resolves a call's method name against the
  implementation. A proxy's own ABI lists only proxy methods, so a call
  through it has to say whose method it means.

When the logic changes, one `labcoat deploy` deploys the new code
and repoints the beacon:

```text
Kind      Name                           Status             Id/Target
contract  auth_token                     complete           4:65517
contract  counter_logic                  pending (replace)  1:0
contract  counter_beacon                 complete           2:2
setting   counter_beacon_implementation  pending (update)   2:2
contract  counter                        complete           2:4
call      init_counter                   complete           2:4

counter_logic: the code changed — deploys a new instance and keeps 2:1 in
the record's history
counter_beacon_implementation: 2:1 → <counter_logic.id>
```

The [upgradeable contract tutorial](/docs/tutorials/upgradeable-contract/)
walks through this from an empty directory.

### Auth tokens

Initializing a standard proxy or beacon mints one **auth token** to the
deploying wallet. Whoever holds it may upgrade that contract; the upgrade
transaction spends it as an input and returns it. Labcoat records the token's
id when the contract deploys (`contract.created(1)`, or `beacon.auth_token()`)
and attaches it to the upgrade for you.

The token is cloned from a template at the reserved id `4:65517`. A fresh
chain has none, which is why the script deploys the `auth-token` package there
first. `.init(100)` calls a read-only opcode, so the template itself is never
initialized.

### Settings

A setting is a mutable on-chain value with a method that writes it and,
ideally, a view that reads it:

```rust
d.setting("market_fee", &market, 30)
    .uint()
    .read("get_fee")
    .write("set_fee");
```

On every plan Labcoat reads the current value and compares it with the
declared one. They match: `complete`. They differ, whether because you changed
the script or because someone changed the chain: `pending (update)`, and
deploy sends the write call and verifies the value before reporting success.

A setting declared for a contract that does not exist yet shows
`pending (check)`. Deploy checks it once the contract is deployed and sends
nothing when the initializer already set the value.

Without `.read(…)`, Labcoat compares against the last successful write and
sends the first write when the current value is unknown. The standard
`d.proxy` uses the implementation argument recorded at deployment until it
has a successful write. A beacon's setting reads the chain.

Labcoat cannot check that a new implementation's storage layout is compatible
with the old one. Test an upgrade on Labcoat Network before deploying it to a
shared network.

## The run file

`deployments/<network>.json` records every step that ran on a network: its
definition, its Alkane id, every Alkane its transaction created, the ids it
referenced, the arguments as executed, the transaction id, and the earlier
instances it replaced. It is what makes a step run once.

- **Commit it for shared networks.** A teammate with the file sees the same
  plan you do; without it, their deploy would run every step again.
- **The Labcoat Network file is disposable.** `labcoat reset` starts a new
  chain, the plan ignores records from the earlier one, and a new project's
  `.gitignore` excludes `deployments/labcoat.json`.

`labcoat.lock` remains the address book that maps a name to its active id for
`labcoat call`, `labcoat simulate`, and
[generated web clients](/docs/web-apps/). `labcoat deploy` updates it as each
contract deploys.

Labcoat records the commit and execution transaction IDs before submission.
Run `labcoat deploy` again after the indexer catches up. It checks
the recorded transaction and continues once its result is known. If the
funding commit was sent and Labcoat saved a prepared reveal, deploy can
continue from that reveal.

If deploy stopped before saving the reveal, check the recorded commit and the
wallet. Use `labcoat import <name> <id> --txid <reveal-txid>` if the
deployment landed. Use `--replace <name>` for a contract or call only after
verifying its previous execution cannot still land. Run one deploy or import
at a time per checkout. When deploying from different checkouts, share the
latest run file and coordinate who deploys.

## Keep usage out of the script

A deployment is finished when its contracts are deployed and inert. Use `call`
steps only for configuration that completes the deployment. Protocol usage,
such as minting, trading, or incrementing a counter, belongs in end-to-end
tests or your application.

## End-to-end tests

```bash
labcoat test --e2e
labcoat test --e2e covered_call --no-reset
```

`labcoat test --e2e` resets Labcoat Network, deploys `deploy.rs`, and runs the
`#[ignore]`d tests in `tests/e2e.rs`. Pass a test-name filter to run a
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

`e2e.contract` resolves the name of any contract or external step,
`e2e.created` returns an Alkane a deploy created (an auth token, for
example), `e2e.wallet` creates funded wallets for multi-party flows, and
`Call` and `Deploy` builders cover inputs, recipients, pointers, and edicts.
For a contract behind a proxy, `Call::abi` and `e2e.simulate_uint_via` name
the implementation whose methods you are calling.

Two complete examples:

- [covered-call](https://github.com/jonatns/labcoat/tree/main/docs/examples/covered-call):
  a four-step deployment with an external reference and a two-wallet protocol
  test.
- [upgradeable-counter](https://github.com/jonatns/labcoat/tree/main/docs/examples/upgradeable-counter):
  a contract behind a beacon proxy.

## Public networks

`plan`, `deploy`, and `import` accept the global `--network`, `--signer`, and
`--yes` options. Non-regtest networks require an explicit signing approval,
and `--signer psbt-file:<dir>` hands every transaction to an external signer,
as described in
[Atomic exchanges](/docs/exchanges/#sign-with-an-external-wallet). The wallet
that deploys an upgrade must hold the contract's auth token.

Mainnet deployment controls are not production-ready; see
[Stability and releases](/docs/reference/stability/).
