---
title: An upgradeable contract
description: Put a contract behind a beacon proxy with a deployment script, then replace its code without changing its id or losing its state.
---

Alkanes contract code cannot change after it is deployed. This tutorial puts
the starter Counter behind a standard **beacon proxy**, so its code can be
replaced while its id and its stored count stay put. You will write a
deployment script, review its plan and deploy it on Labcoat Network, deploy
an upgrade, and see what Labcoat refuses to change.

You need a [Labcoat installation](/docs/getting-started/installation/) and
about fifteen minutes. The first build compiles the Alkanes runtime and takes
the longest.

## How the pieces fit

Three contracts stand between a caller and your code:

```text
caller ──▶ counter (beacon proxy)        holds the count; id never changes
               │ asks "which code?"
               ▼
           counter_beacon                holds one id: the current logic
               │
               ▼
           counter_logic                 your code; replaced on upgrade
```

The proxy keeps the storage and delegates every call to whatever the beacon
points at. Upgrading means deploying new logic and pointing the beacon at it.
Whoever holds the beacon's **auth token** is allowed to do that.

## Create the project

```bash
labcoat init upgrade-lab
cd upgrade-lab
labcoat new auth-token --template auth-token
labcoat new beacon --template upgradeable-beacon
labcoat new beacon-proxy --template beacon-proxy
```

`labcoat init` scaffolds the Counter contract, which becomes the logic. Each
`labcoat new --template` vendors one of the standard Alkanes contracts into
`contracts/` as an ordinary package you can read:

| Package | What it is |
|---|---|
| `auth-token` | The token that authorizes upgrades. |
| `beacon` | The upgradeable beacon. |
| `beacon-proxy` | The proxy that follows a beacon. |

Check that everything builds and the Counter's own tests still pass:

```bash
labcoat test
```

## Write the deployment script

Replace `deploy.rs`:

```rust
use labcoat::{run, Deployment};

fn main() {
    run(|d: &Deployment| {
        // Every standard proxy and beacon clones its auth token from
        // 4:65517. A fresh chain has none, so deploy the template first.
        // Opcode 100 only reads the symbol: the template itself is never
        // initialized.
        d.contract("auth_token", "auth-token").reserve(65517).init(100);

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
        d.call("init_counter", &counter, "initialize").abi_of(&logic);
    });
}
```

The script declares five steps and executes none of them. Two details matter:

- **The logic is an `implementation`, not a `contract`.** A contract is
  deployed once and then frozen. An implementation is the one kind of step
  Labcoat redeploys when its code changes.
- **The Counter is initialized through the proxy.** The count lives in the
  proxy's storage, so `initialize` has to run there. `abi_of(&logic)` tells
  Labcoat the method name belongs to the logic: the proxy's own ABI knows only
  proxy methods.

## Start the network and fund the wallet

```bash
labcoat up
labcoat wallet init
labcoat wallet addresses
```

Fund the displayed address and confirm it:

```bash
labcoat fund <address>
labcoat mine 1
```

## Review the plan

```bash
labcoat plan
```

```text
› Plan for deploy.rs — 6 pending step(s)

Kind      Name                           Status            Id/Target
contract  auth_token                     pending (create)  4:65517
contract  counter_logic                  pending (create)  1:0
contract  counter_beacon                 pending (create)  1:0
setting   counter_beacon_implementation  pending (check)   -
contract  counter                        pending (create)  1:0
call      init_counter                   pending (call)    -

Step                           Arguments
counter_beacon                 implementation=<counter_logic.id>, auth_token_units=1
counter_beacon_implementation  <counter_logic.id>
counter                        beacon=<counter_beacon.id>
```

Planning signs nothing. It shows what will run and with which arguments.
`<counter_logic.id>` is a reference that resolves once that step has run;
`4:65517` is known already because a reserved id is predictable.

`d.beacon` declared two steps: the beacon contract, and a **setting** named
`counter_beacon_implementation` that keeps the beacon pointing at the current
logic. It shows `pending (check)` because there is nothing to compare yet.

## Deploy it

```bash
labcoat deploy --broadcast
```

```text
✓ Executed 5 of 6 step(s)

Kind      Name                           Outcome   Id       Change
contract  auth_token                     executed  4:65517
contract  counter_logic                  executed  2:1
contract  counter_beacon                 executed  2:2
setting   counter_beacon_implementation  in-sync   2:2      already 2:1
contract  counter                        executed  2:4
call      init_counter                   executed  2:4
```

Five transactions went out. The setting needed none: Labcoat read the beacon,
found it already pointing at `2:1`, and recorded that.

The beacon is `2:2` and the proxy is `2:4`. The id in between, `2:3`, is the
beacon's auth token, minted to your wallet when the beacon initialized.

Run `labcoat plan` again and every step is `complete`. Deploying an unchanged
script does nothing.

## Use the contract

Call the proxy, and name the logic whose methods you mean with `--abi`:

```bash
labcoat simulate counter get_count --abi counter_logic
labcoat call counter increment --abi counter_logic
labcoat simulate counter get_count --abi counter_logic
```

The count goes from `0` to `1`. It is stored in `counter`, the proxy.

## Upgrade the logic

Add a method to the logic. In `contracts/counter/src/lib.rs`, add a variant to
`CounterMessage`:

```rust
    #[opcode(2)]
    #[returns(u128)]
    Add { amount: u128 },
```

and its handler to `impl Counter`:

```rust
    fn add(&self, amount: u128) -> Result<CallResponse> {
        let mut pointer = self.count_pointer();
        let count = pointer
            .get_value::<u128>()
            .checked_add(amount)
            .ok_or_else(|| anyhow!("counter overflow"))?;
        pointer.set_value::<u128>(count);
        self.response_with_count(count)
    }
```

You did not touch `deploy.rs`. Plan again:

```bash
labcoat plan
```

```text
› Plan for deploy.rs — 2 pending step(s)

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

Labcoat noticed the logic's code changed. Because it is an implementation, the
plan deploys a new instance and repoints the beacon. The beacon and the proxy
are untouched.

```bash
labcoat deploy --broadcast
```

```text
✓ Executed 2 of 6 step(s)

Kind      Name                           Outcome           Id       Change
contract  auth_token                     skipped-complete  4:65517
contract  counter_logic                  executed          2:5
contract  counter_beacon                 skipped-complete  2:2
setting   counter_beacon_implementation  executed          2:2      2:1 → 2:5
contract  counter                        skipped-complete  2:4
call      init_counter                   skipped-complete  2:4
```

The upgrade transaction spent the auth token and returned it to your wallet.
Now check the contract:

```bash
labcoat simulate counter get_count --abi counter_logic
labcoat call counter add 41 --abi counter_logic
labcoat simulate counter get_count --abi counter_logic
```

The count is still `1` after the upgrade, and `42` after calling the new
method. Same id, same state, new code.

`deployments/labcoat.json` holds the record. `counter_logic` now has id `2:5`
and a `history` entry for `2:1`, which still exists on chain; nothing points
at it any more.

## See what Labcoat will not change

The beacon is deployed once. Try to edit it: in `deploy.rs`, mint two auth
tokens instead of one.

```rust
        let beacon = d.beacon("counter_beacon", "beacon", &logic).auth_units(2);
```

```bash
labcoat plan
```

```text
! Plan for deploy.rs — 0 pending, 1 in conflict

Kind      Name                           Status    Id/Target
contract  auth_token                     complete  4:65517
contract  counter_logic                  complete  2:5
contract  counter_beacon                 conflict  2:2
setting   counter_beacon_implementation  complete  2:2
contract  counter                        complete  2:4
call      init_counter                   complete  2:4

Step            Arguments
counter_beacon  implementation=2:5, auth_token_units=2

counter_beacon: its definition changed since it ran — restore it, or pass
`--replace counter_beacon` to deploy a new instance

Deploy refuses while a step is in conflict: restore the step, or name it in
--replace.
```

A beacon that already initialized cannot mint a different number of tokens, so
there is nothing Labcoat could do to make the chain match. It says so instead
of pretending, and `labcoat deploy --broadcast` stops with
`DEPLOYMENT_CONFLICT` before sending anything.

`--replace counter_beacon` would deploy a second beacon with a new id. The
proxy points at the first one, so the plan would then list `counter` as a
conflict too. That is the real cost of replacing a pinned contract, shown
before you pay it. Here, undo the edit instead:

```rust
        let beacon = d.beacon("counter_beacon", "beacon", &logic);
```

and the plan is clean again.

## Test the deployment

`labcoat test --e2e` resets Labcoat Network, deploys `deploy.rs` from scratch,
and runs `tests/e2e.rs`. Replace that file:

```rust
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
```

```bash
labcoat test --e2e
```

## Going further

- **More contracts behind one beacon.** Declare another
  `d.beacon_proxy("counter_b", "beacon-proxy", &beacon)`. One beacon upgrade
  then changes the code of every proxy that follows it.
- **A contract with its own pointer.** `labcoat new proxy --template
  upgradeable` and `d.proxy("vault", "proxy", &logic)` declare a standalone
  upgradeable proxy with no beacon.
- **Shared networks.** Pass `--network signet` to `plan` and `deploy`, and
  commit `deployments/signet.json`: it is the record that keeps a second
  deploy, by you or a teammate, from running every step again. The wallet
  that deploys an upgrade has to hold the auth token. If the auth token
  template already exists at `4:65517`, the plan adopts it instead of
  deploying it.
- **Storage compatibility is yours to check.** Labcoat cannot tell whether new
  logic reads the old logic's storage correctly. Upgrade on Labcoat Network
  and test before upgrading anywhere that matters.

## Clean up

```bash
labcoat down
```

## Next steps

- [Deployment scripts](/docs/deployments/) covers every step kind, external
  references, `labcoat import`, and the run file.
- The [upgradeable-counter example](https://github.com/jonatns/labcoat/tree/main/docs/examples/upgradeable-counter)
  has this tutorial's finished `deploy.rs` and `tests/e2e.rs`.
- [Tokens and exchanges](/docs/tutorials/token-exchange/) builds a token and
  swaps it atomically between two wallets.
