---
title: Tokens and exchanges
description: Write a fixed-supply token, deploy it from two wallets, and swap the two tokens atomically.
---

This tutorial builds a minimal fixed-supply token, deploys two copies of it
from two different wallets on Labcoat Network, and swaps them in one
transaction: first with the local one-command exchange, then with the PSBT
ceremony that public networks use.

You need a [Labcoat installation](/docs/getting-started/installation/) and
about fifteen minutes. The first build compiles the Alkanes runtime and takes
the longest.

## Create the project

```bash
labcoat init swap-lab
cd swap-lab
labcoat new token
```

`labcoat new token` adds `contracts/token/` and a host test in
`tests/token.rs`.

## Write the token

Replace `contracts/token/src/lib.rs`:

```rust
//! Fixed-supply token.
//!
//! - `0`: initialize, minting `supply` units to the deployer
//! - `99`: name and `100`: symbol, as UTF-8 text
//! - `101`: the configured supply

use alkanes_runtime::{
    declare_alkane, message::MessageDispatch, runtime::AlkaneResponder, storage::StoragePointer,
};
use alkanes_support::{parcel::AlkaneTransfer, response::CallResponse};
use anyhow::Result;
use metashrew_support::{compat::to_arraybuffer_layout, index_pointer::KeyValuePointer};

#[derive(Default)]
pub struct Token(());

#[derive(MessageDispatch)]
enum TokenMessage {
    #[opcode(0)]
    Initialize { supply: u128 },

    #[opcode(99)]
    #[returns(String)]
    GetName,

    #[opcode(100)]
    #[returns(String)]
    GetSymbol,

    #[opcode(101)]
    #[returns(u128)]
    GetSupply,
}

impl Token {
    fn supply_pointer(&self) -> StoragePointer {
        StoragePointer::from_keyword("/supply")
    }

    fn respond(&self, data: Vec<u8>) -> Result<CallResponse> {
        let context = self.context()?;
        let mut response = CallResponse::forward(&context.incoming_alkanes);
        response.data = data;
        Ok(response)
    }

    fn initialize(&self, supply: u128) -> Result<CallResponse> {
        self.observe_initialization()?;
        let context = self.context()?;
        self.supply_pointer().set_value::<u128>(supply);
        let mut response = CallResponse::forward(&context.incoming_alkanes);
        response.alkanes.0.push(AlkaneTransfer {
            id: context.myself.clone(),
            value: supply,
        });
        Ok(response)
    }

    fn get_name(&self) -> Result<CallResponse> {
        self.respond(b"Test Token".to_vec())
    }

    fn get_symbol(&self) -> Result<CallResponse> {
        self.respond(b"TT".to_vec())
    }

    fn get_supply(&self) -> Result<CallResponse> {
        let supply = self.supply_pointer().get_value::<u128>();
        self.respond(supply.to_le_bytes().to_vec())
    }
}

impl AlkaneResponder for Token {}

declare_alkane! {
    impl AlkaneResponder for Token {
        type Message = TokenMessage;
    }
}
```

Initialization mints the whole supply as an Alkane transfer of the contract's
own ID, which lands on the deploying wallet's output.

Opcodes `99` and `100` follow the Alkanes metadata convention. Balance views
ask every held token for its name and symbol, so a token that answers those
opcodes with anything other than UTF-8 text breaks `labcoat balance` and
exchanges for every address that holds it.

## Test it natively

Replace `tests/token.rs`:

```rust
use labcoat_test::{assert_revert, ContractHarness, Value};

#[test]
fn mints_the_supply_to_the_deployer() -> Result<(), Box<dyn std::error::Error>> {
    let mut token = ContractHarness::for_contract("token")?;

    let minted = token.call_method("initialize", &[Value::U128(1000)])?;
    assert_eq!(minted.transfers.len(), 1);
    assert_eq!(minted.transfers[0].value, 1000);

    assert_eq!(token.call_method("get_name", &[])?.data_text(), "Test Token");
    assert_eq!(token.call_method("get_symbol", &[])?.data_text(), "TT");

    // A token initializes exactly once.
    assert_revert(token.call_method("initialize", &[Value::U128(1)]));
    Ok(())
}
```

```bash
labcoat test token
```

## Start the network and create a second wallet

The network's built-in wallet plays the buyer. A second keystore file plays the seller.

```bash
labcoat up
labcoat --wallet-file seller.json wallet init
```

`labcoat up` prints the buyer's address, already funded. `wallet init` prints
the seller's. Fund it and mine a block:

```bash
labcoat fund <seller-address> 2
labcoat mine 1
```

## Deploy two tokens

Build once, then deploy the same Wasm twice under different names: the seller
deploys `alpha` and the buyer deploys `beta`.

```bash
labcoat build token
labcoat --wallet-file seller.json deploy --wasm build/token.wasm --name alpha --args 1000
labcoat deploy --wasm build/token.wasm --name beta --args 5000
```

`--args` fills the constructor's `supply` parameter. Both names are now in
`labcoat.lock`; check the balances:

```bash
labcoat balance <seller-address>
labcoat balance <buyer-address>
```

The seller holds 1,000 `alpha` and the buyer holds 5,000 `beta`.

## Swap with one command

On Labcoat Network, one command plans the exchange, signs with both
keystores, and broadcasts. The seller gives 100 `alpha` and receives 250
`beta`:

```bash
labcoat exchange alpha 100 beta 250 --seller-wallet-file seller.json
labcoat mine 1
labcoat balance <seller-address>
labcoat balance <buyer-address>
```

The seller now holds 900 `alpha` and 250 `beta`; the buyer holds 100 `alpha`
and 4,750 `beta`. Each output the exchange created holds a single asset: the
seller's unsold 900 `alpha` returned in its own change output, so it can be
sold again.

## Swap with the PSBT ceremony

On public networks no single process should hold both keys. The ceremony
splits the same exchange into three steps, each run with one party's keystore.
Sell 50 more `alpha` for 125 `beta`, spending the change from the first swap:

```bash
# Buyer: plan the exchange and write the unsigned PSBT.
labcoat exchange-plan alpha 50 beta 125 \
  --seller-address <seller-address> --buyer-address <buyer-address> \
  --plan-out plan.json --psbt-out exchange.psbt

# Buyer: sign the buyer's inputs.
labcoat wallet sign-psbt --in exchange.psbt --out exchange.buyer-signed.psbt

# Seller: check the PSBT against the plan, sign last, and broadcast.
labcoat exchange-settle --plan plan.json --psbt exchange.buyer-signed.psbt \
  --seller-wallet-file seller.json --broadcast

labcoat mine 1
```

`plan.json` lists every output with its owner and asset. Before signing
anything, `exchange-settle` verifies that the PSBT matches that plan: the
buyer receives exactly 50 `alpha`, the seller exactly 125 `beta`, each side's
surplus returns to its owner, and the seller loses no bitcoin. The seller
signs last, so it never hands over a fully signed transaction before
broadcasting it.

Afterwards the seller holds 850 `alpha` and 375 `beta`, and the buyer holds
150 `alpha` and 4,625 `beta`.

## Clean up

```bash
labcoat down
```

`labcoat down` keeps the chain for next time; `labcoat reset -y` wipes it.

## Next steps

- [Atomic exchanges](/docs/exchanges/) covers request files and external
  signers.
- [Web apps](/docs/web-apps/) generates a browser client that can read these
  balances and prepare exchange requests.
- [Deployment scripts](/docs/deployments/) deploys tokens like these from
  `deploy.rs`, with `reserve` IDs you know in advance.
- [An upgradeable contract](/docs/tutorials/upgradeable-contract/) puts a
  contract behind a proxy and replaces its code without changing its id.
