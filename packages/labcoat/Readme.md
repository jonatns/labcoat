# 👨‍🔬 Labcoat ⚛

Labcoat is a development toolkit for **Bitcoin Alkanes smart contracts**. It provides a Hardhat-style experience for compiling, deploying, and testing Alkanes contracts.

---

## Features

- Compile Alkanes contracts (`.rs`) to WebAssembly (`.wasm`)
- Generate ABI from Rust contracts automatically
- Deploy contracts through the Rust core built on the pinned alkanes-rs develop commit (no oyl-sdk)

---

## Installation

First, create a new directory for your project:

```bash
mkdir labcoat-example
cd labcoat-example
```

Once that's done, initialize your Labcoat project by running:

```bash
npx @jonatns/labcoat init
```

Then install the required dependencies:

```bash
npm i
```

## Project structure

```bash
labcoat.config.ts

contracts
├── Example.rs
├── Storage.rs

deployments
├── manifest.json

scripts
└── example.ts
└── storage.ts
```

## Writing a smart contract

Writing a smart contract with Labcoat is as easy as writing a Rust file inside the contracts directory. For example, your contracts/Example.rs should look like this:

```rust
use alkanes_runtime::{declare_alkane, message::MessageDispatch, runtime::AlkaneResponder};
use alkanes_support::response::CallResponse;
use anyhow::Result;
use metashrew_support::compat::to_arraybuffer_layout;

#[derive(Default)]
pub struct ExampleContract(());

#[derive(MessageDispatch)]
enum ExampleContractMessage {
    #[opcode(0)]
    Initialize,

    #[opcode(1)]
    #[returns(String)]
    Greet { name: String },
}

impl ExampleContract {
    fn initialize(&self) -> Result<CallResponse> {
        let context = self.context()?;
        let response = CallResponse::forward(&context.incoming_alkanes);
        Ok(response)
    }

    fn greet(&self, name: String) -> Result<CallResponse> {
        let context = self.context()?;
        let mut response = CallResponse::forward(&context.incoming_alkanes);

        let message = format!("Hello {}!", name);
        response.data = message.as_bytes().to_vec();

        Ok(response)
    }
}

impl AlkaneResponder for ExampleContract {}

declare_alkane! {
    impl AlkaneResponder for ExampleContract {
        type Message = ExampleContractMessage;
    }
}
```

Then all you need to do is run:

```bash
npx labcoat compile contracts/Example.rs
```

## Deploying contracts

A script in Labcoat is just a TypeScript file with access to your contracts, configuration, and any other functionality that Labcoat provides. You can use them to run deploy scripts or custom logic like simulations and executions.

### Writing a Labcoat script (scripts/example.ts):

```typescript
import { labcoat } from "@jonatns/labcoat";

export default async function main() {
  const { deploy, simulate } = await labcoat.setup();

  await deploy("Example");

  const result = await simulate("Example", "Greet", ["World"]);
  console.log("📦 Result:", result);
}

main()
  .then(() => process.exit(0))
  .catch((err) => {
    console.error("❌", err);
    process.exit(1);
  });
```

### Setting up a wallet:

Before deploying you can set a mnemonic in `.env` used by `labcoat.config.ts`. By default labcoat targets the local regtest devnet (`labcoat up`):

```bash
export default {
  network: "regtest",
  mnemonic: process.env.MNEMONIC,
};

```

Once that's done run the example script by using our generic run command:

```bash
npx labcoat run scripts/example.ts
```

The example script will deploy and simulate a function call against the Example contract. The Tx ID and Alkane ID will be stored in deployments/manifest.json for future use.
