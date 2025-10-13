# Labcoat

Labcoat is a development toolkit for **Bitcoin Alkanes smart contracts**. It provides a Hardhat-style experience for compiling, deploying, and testing Alkanes contracts.

---

## Features

- Compile Alkanes contracts (`.rs`) to WebAssembly (`.wasm`)
- Generate ABI from Rust contracts automatically
- Deploy contracts to Bitcoin networks using `oyl-sdk`

---

## Installation

First, create a new directory for your project:

```bash
mkdir labcoat-example
cd labcoat-example
```

Once that's done, initialize your Labcoat project by running:

```bash
npx labcoat init
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

scripts
└── deploy.ts
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
    DoSomething,
}

impl ExampleContract {
    fn initialize(&self) -> Result<CallResponse> {
        let context = self.context()?;
        let response = CallResponse::forward(&context.incoming_alkanes);
        Ok(response)
    }

    fn do_something(&self) -> Result<CallResponse> {
        let context = self.context()?;
        let mut response = CallResponse::forward(&context.incoming_alkanes);
        response.data = b"Hello from ExampleContract".to_vec();
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
npx labcoat compile
```

## Deploying contracts using scripts

A script in Labcoat is just a TypeScript file with access to your contracts, configuration, and any other functionality that Labcoat provides. You can use them to run deploy scripts or custom logic like simulations and executions.

#### Example deploy script (scripts/deploy.ts):



```typescript
import { labcoat } from "@jonatns/labcoat";

export default async function main() {
  const { deploy } = await labcoat.setup();
  await deploy("Example");
}

main().catch((err) => {
  console.error("❌ Deployment failed:", err);
  process.exit(1);
});
```

Then run:

```bash
npx labcoat run scripts/deploy.ts
```

Labcoat will print the TX ID along with the Alkanes ID once the TX is confirmed. The TX ID and Alkanes ID are stored in the generated ABI for future use.