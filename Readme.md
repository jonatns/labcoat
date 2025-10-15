# 👨‍🔬 Labcoat ⚛

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

deployments
├── manifest.json

scripts
└── deploy.ts
└── greet.ts
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
npx labcoat compile
```

## Deploying contracts

A script in Labcoat is just a TypeScript file with access to your contracts, configuration, and any other functionality that Labcoat provides. You can use them to run deploy scripts or custom logic like simulations and executions.

### Writing a deploy script (scripts/deploy.ts):

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

### Setting up a wallet:

Before deploying to the Bitcoin network you will need to setup a mnemonic in `.env` which is used by `labcoat.config.ts`. By default it uses the oylnet network:

```bash
export default {
  network: "oylnet",
  mnemonic: process.env.MNEMONIC,
};

```

Once that's done run the deploy script by using our generic run command:

```bash
npx labcoat run scripts/deploy.ts
```

Labcoat will print the Tx ID along with the Alkanes ID once the TX is confirmed. The Tx ID and Alkanes ID are stored in deployments/manifest.json for future use.

### Simulating contract calls (scripts/greet.ts)

You can simulate contracts calls by creating a script and using the simulate function returned by `labcoat.setup()`:

```typescript
import { labcoat } from "@jonatns/labcoat";

export default async function main() {
  const { simulate } = await labcoat.setup();
  await simulate("Example", "Greet", ["World"]);
}

main().catch((err) => {
  console.error("❌ Deployment failed:", err);
  process.exit(1);
});
```

The script above makes a call to a method called `Greet` in the `Example` contract. It also passes an argument with the word `World`. 

You should see the following result when running:

```bash
npx labcoat run scripts/greet.ts
```

```json
{
  "status": 0,
  "gasUsed": 39656,
  "execution": {
    "alkanes": [],
    "storage": [],
    "error": null,
    "data": "0x48656c6c6f20576f726c6421"
  },
  "parsed": {
    "string": "Hello World!",
    "bytes": "0x48656c6c6f20576f726c6421",
    "le": "10334410032597741434076685640",
    "be": "22405534230753928650781647905"
  }
}
```

