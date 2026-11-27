---
title: Your first contract
description: Read the starter Counter, deploy it to Labcoat Network, call it, then add a method and deploy the change.
---

This tutorial takes the Counter contract every Labcoat project starts with
and runs it through the whole loop: test, deploy, call, trace, change, and
deploy again. It assumes you know some Rust and nothing about Alkanes.

You need a [Labcoat installation](/docs/getting-started/installation/), its
[compilation prerequisites](/docs/getting-started/installation/#compilation-prerequisites)
(`labcoat doctor` checks them), and about ten minutes. The first build
compiles the Alkanes runtime and takes the longest.

## Create the project

```bash
labcoat init first-alkane
cd first-alkane
```

The files that matter here:

```text
contracts/counter/src/lib.rs   the contract
tests/counter.rs               tests that run without a chain
deploy.rs                      what gets deployed
```

## Read the contract

Open `contracts/counter/src/lib.rs`. An Alkanes contract is a Rust library
compiled to WebAssembly. Three parts make it up.

**Messages** are the contract's public methods. Each has an opcode, the
number a caller sends to select it:

```rust
#[derive(MessageDispatch)]
enum CounterMessage {
    #[opcode(0)]
    Initialize,

    #[opcode(1)]
    #[returns(u128)]
    Increment,

    #[opcode(20)]
    #[returns(u128)]
    GetCount,
}
```

Labcoat reads these into an ABI, so you call methods by name: `increment`,
`get_count`.

**Storage** is a key-value store that belongs to the contract. The count
lives under one key:

```rust
fn count_pointer(&self) -> StoragePointer {
    StoragePointer::from_keyword("/count")
}
```

**Handlers** are one function per message. `increment` reads the count, adds
one, writes it back, and returns it:

```rust
fn increment(&self) -> Result<CallResponse> {
    let mut pointer = self.count_pointer();
    let count = pointer
        .get_value::<u128>()
        .checked_add(1)
        .ok_or_else(|| anyhow!("counter overflow"))?;
    pointer.set_value::<u128>(count);
    self.response_with_count(count)
}
```

Returning an error reverts the call: nothing it wrote is kept.

## Test it without a chain

```bash
labcoat test
```

```text
✓ Tests passed
```

`tests/counter.rs` runs the compiled contract in an in-memory harness, so
tests take seconds and need no network:

```rust
let mut contract = ContractHarness::for_contract("counter")?;
contract.call_method("initialize", &[])?;

let first = contract.call_method("increment", &[])?;
assert_eq!(returned_count(&first.data)?, 1);
```

## Start the network

```bash
labcoat up
```

Labcoat Network is a private Bitcoin chain on your machine. It comes with a
funded wallet, so there is nothing else to set up.

## Deploy

`deploy.rs` declares what the project deploys. A new project's declares the
Counter:

```rust
d.contract("counter", "counter");
```

The first name is what you will call the deployed contract. The second is the
Cargo package to build. Look at the plan, then deploy:

```bash
labcoat plan
labcoat deploy
```

```text
✓ Executed 1 of 1 step(s)

Kind      Name     Outcome   Id   Change
contract  counter  executed  2:1
```

`2:1` is the contract's Alkane ID. Labcoat saves it under the name `counter`,
so you never type the ID. Run `labcoat deploy` again and nothing happens: a
step that already ran is skipped.

## Call it

`simulate` runs a method against the current chain state and changes nothing.
`call` sends a transaction.

```bash
labcoat simulate counter get_count
labcoat call counter increment
labcoat simulate counter get_count
```

```text
✓ Call succeeded

Target       2:1
Method       increment
Opcode       1
Transaction  54a2a0c2806d4a68b602ca0c61e9835894dcc130333f8e78828ffd987a299c0c
Fee          368 sats
Return       1
```

The count goes from `0` to `1`. To see what the transaction did inside the
contract, trace it with the ID from your own output:

```bash
labcoat trace <txid>
```

```text
• Protostone 1
  • call 0:0 -> 2:1  inputs [1, …]
  • return 1
```

## Add a method

Give the Counter an `add` method. In `contracts/counter/src/lib.rs`, add a
message to `CounterMessage`:

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

Test it before deploying. In `tests/counter.rs`, change the import to

```rust
use labcoat_test::{ContractHarness, Value};
```

and add a test:

```rust
#[test]
fn adds_an_amount() -> Result<(), Box<dyn std::error::Error>> {
    let mut contract = ContractHarness::for_contract("counter")?;
    contract.call_method("initialize", &[])?;

    let added = contract.call_method("add", &[Value::U128(41)])?;
    assert_eq!(returned_count(&added.data)?, 41);
    Ok(())
}
```

```bash
labcoat test
```

## Deploy the change

```bash
labcoat deploy
```

```text
✓ Executed 1 of 1 step(s)

Kind      Name     Outcome     Id   Change
contract  counter  redeployed  2:2  2:1 → 2:2

A redeployed contract has a new id and empty storage; the earlier one stays on chain.
```

A deployed contract's code can never change. So Labcoat deployed your new
code as a second contract, `2:2`, and pointed the name `counter` at it. Its
count starts at `0` again; the old one at `2:1` still exists with its count of
`1`.

Labcoat only does this on its own on Labcoat Network, where nothing is worth
keeping. On a shared network the same edit stops with a conflict until you
ask for a replacement.

```bash
labcoat call counter add 41
labcoat simulate counter get_count
```

The count is `41`.

## Clean up

```bash
labcoat down
```

`labcoat down` keeps the chain for next time; `labcoat reset -y` wipes it.

## Next steps

- [A web app](/docs/tutorials/web-app/) puts a page in front of this Counter
  that reads the count and changes it with a signed transaction.
- [Tokens and exchanges](/docs/tutorials/token-exchange/) writes a token and
  swaps it between two wallets.
- [An upgradeable contract](/docs/tutorials/upgradeable-contract/) changes a
  contract's code while keeping its ID and its stored state.
- [Deployment scripts](/docs/deployments/) covers everything `deploy.rs` can
  declare.
