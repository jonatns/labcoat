---
name: labcoat
description: Labcoat is the Rust-native CLI for building, testing, and operating Alkanes smart contracts on Labcoat Network, a managed local Bitcoin regtest. Use when working in a Labcoat project or developing Alkanes contracts.
---

# Labcoat: the Alkanes contract workflow

New projects start with `labcoat init <project-name>` (or `labcoat init` for an
interactive prompt). Native integration
tests live under `tests/`; use `labcoat new <name>` to add a minimal
contract package and matching test without copying the example.
Run integration tests with `labcoat test`.

Every command supports `--json` and then emits exactly one envelope on
stdout (`{ok, command, schema, result | error{code, message, hint}}`),
logs on stderr, exit 0 whenever an envelope was printed. On any error,
`error.hint` names the next command to run — follow it.

## 1. Boot infrastructure

```bash
labcoat up --json          # prepares this CLI version's runtime, boots the stack
labcoat status --json      # poll until result.is_ready == true
```

`up` returns `result.endpoints.qubitcoin_rpc`, the direct local Qubitcoin
endpoint (`http://127.0.0.1:18443`).

## 2. Wallet

```bash
labcoat wallet init --json                 # creates .labcoat/wallet.json
labcoat wallet addresses --json            # p2tr address is the primary
labcoat fund <p2tr-address> --json         # faucet 1 BTC
labcoat mine 1 --json                      # confirm it
labcoat wallet utxos --json                # verify spendable balance
```

Secrets: `LABCOAT_WALLET_PASSPHRASE` env (Labcoat Network and custom regtest
have a development default);
mnemonic via `LABCOAT_MNEMONIC` env or `wallet init --mnemonic-stdin`.
Never place either on argv.

## 3. Build

```bash
labcoat build my-token --json
```

Result: `build/my-token.wasm` (raw — what deploy consumes),
`build/my-token.wasm.gz`, `build/my-token.abi.json`. The ABI is extracted
from the compiled Wasm's `__meta` export. Commit the `Cargo.lock` created by
the first build.

## 4. Deploy

```bash
labcoat plan --json                          # what would run; never signs
labcoat deploy --broadcast --json            # run every pending deploy.rs step
labcoat deploy my-token --broadcast --json   # run one step
```

`deploy.rs` declares what the project deploys (described below); a new
project's declares its Counter. Deploy recompiles each package, sends a
commit/reveal envelope per contract, waits for the `create` trace, and
records each id in `labcoat.lock` under the current network. Each step's
`outcome`, `id` (`block:tx`), and `txid` are in `result.actions`. A step runs
once per network, so deploying again does nothing until a step is pending.

Deploy never broadcasts by default: pass `--broadcast` to sign and send, or
`--dry-run` to get the plan. With neither it fails with `BROADCAST_REQUIRED`.
To deploy a contract `deploy.rs` does not declare, add a step for it. Use
`labcoat deploy --wasm <raw-file.wasm> --name <name> --broadcast` only for a
one-off artifact outside the deployment; it is recorded in `labcoat.lock`
alone and returns `result.alkanesId`.

## 5. Call & simulate

```bash
labcoat simulate counter get_count --json    # read-only; decoded result
labcoat call counter increment --json        # state-changing; auto-mines
labcoat call my-token mint 500 --json        # ABI-typed u128 parameter
```

Contract references: the labcoat.lock name (`my-token`) or a raw
`block:tx` id. Use an exact ABI method name with one shell argument per
parameter; `u128`, arbitrary UTF-8 `String`, and decimal `block:tx`
`AlkaneId` values are encoded for the deployed contract. A matching generated
build ABI avoids the metadata request; a different local build produces a
warning and the deployed ABI is used. A numeric opcode keeps the raw cellpack
format for advanced calls. Simulation always uses deployed code and live
indexed state; use `labcoat test <package>` for an undeployed local build.
`result.status` is `success` or `revert` (with `result.revertReason` decoded).
`call` and `deploy --wasm` accept transaction shaping: `--inputs
block:tx:amount` (incoming alkanes; `B:sats` for bitcoin), `--to <address>`
(protostone recipient), `--pointer`/`--refund` (`vN`/`pN` routing), repeated
`--edict block:tx:amount:target`, and `deploy --wasm --reserve N` targets
`[3,N]`. A `deploy.rs` step declares the same options in the script.
`labcoat balance <address> --json` lists alkanes token balances.

Declare the deployment in `deploy.rs`, the project's `deploy` Cargo
example target, with `labcoat`. The script only declares steps:
`labcoat plan` runs it to read them (no signer), and `labcoat deploy
--broadcast` executes what has not run. Steps run in declaration order and
reference only earlier steps.

```rust
use labcoat::{run, Deployment};

fn main() {
    run(|d: &Deployment| {
        let usd = d.external("usd").on("labcoat", 4, 65012).on("signet", 2, 190213);
        let token = d.contract("token", "my-token").reserve(65011).arg("supply", 100);
        d.contract("market", "my-market")
            .arg("base", token.id())
            .arg("quote", usd.id())
            .arg("expiry", d.height() + 100);
    });
}
```

- `d.contract(name, package)` deploys once; `.reserve(n)` targets `4:n`,
  `.init(method)` names the initializer (ABI name or opcode; default is the
  opcode-0 constructor), `.arg(name, value)` passes named args matched to ABI
  parameters, `.positional(value)` passes raw ones. `d.contract_wasm(name,
  path)` deploys a prebuilt wasm; `d.clone_of(name, &template)` clones
  deployed code.
- `d.external(name)` names an Alkane that already exists (`.on(network,
  block, tx)` or `.id_everywhere(block, tx)`); plan fails fast on a network
  with no binding.
- `d.call(name, &contract, method)` runs once, with `.arg`, `.inputs`, `.to`,
  `.pointer`, `.refund`, `.edict`. Reserve calls for configuration that
  completes the deployment; value-moving, actor-specific operations (funding,
  transfers, exercising) belong in `tests/e2e.rs` or application flows.
- Values: `step.id()`, `.block()`, `.tx()`, `.amount(n)`,
  `contract.created(i)`, and `d.height() + n` (evaluated once, when the step
  runs).

A contract, clone, or call is pinned: it runs once per network and is
recorded in `deployments/<network>.json`. Editing it afterwards, or replacing
something it references, is a `conflict`, and deploy stops with
`DEPLOYMENT_CONFLICT` before broadcasting. Do not work around a conflict:
restore the step, or pass `--replace <name>` (a new instance with a new id;
every step that referenced the old one must be replaced too), or on Labcoat
Network `labcoat reset`. `labcoat import <name> <block:tx>` records a
contract that already exists without deploying.

Upgradeable contracts use the standard proxies. Vendor them with `labcoat new
<name> --template auth-token|upgradeable|upgradeable-beacon|beacon-proxy`,
then:

```rust
d.contract("auth_token", "auth-token").reserve(65517).init(100);
let logic = d.implementation("counter_logic", "counter");
let beacon = d.beacon("counter_beacon", "beacon", &logic);
let counter = d.beacon_proxy("counter", "beacon-proxy", &beacon);
d.call("init_counter", &counter, "initialize").abi_of(&logic);
```

`d.implementation` is the only replaceable step: changed code deploys a new
instance, and the beacon's `<name>_implementation` setting repoints it in the
same deploy, spending and returning the auth token the deploying wallet holds.
`d.proxy(name, package, &logic)` is the standalone proxy. Call through a
proxy with `--abi <implementation>` (`labcoat call counter increment --abi
counter_logic`): the proxy's own ABI lists only proxy methods.

Imperative multi-step flows belong in `tests/e2e.rs` (`labcoat test --e2e`),
which resets the chain, deploys `deploy.rs`, and runs `#[ignore]`d Rust
tests using `labcoat_test::e2e::E2e` (disposable wallets, calls, balances,
mining).

Swap two Alkane assets atomically between two wallets in one transaction.
On Labcoat Network one command signs with both keystores (the project
wallet is the buyer); on public networks use the seller-last PSBT ceremony
`exchange-plan` → `wallet sign-psbt` → `exchange-settle --broadcast`, where
each party signs only its own inputs:

```bash
labcoat exchange token 100 usd 7500 --seller-wallet-file seller.json --json
labcoat exchange-plan --request exchange-request.json --plan-out plan.json --psbt-out exchange.psbt --json
```

`labcoat generate web` writes a typed, read-only TypeScript client
(`generated/web/`: manifest, ABI descriptors, `simulate`, balances, and an
exchange request builder) from `labcoat.lock` and `build/*.abi.json` with
no network access; regenerate after every deploy or reset.
`labcoat state migrate` opts a project into durable deployment history
(`state list`, `state show <name> --history`); afterwards a deploy onto a
reset chain fails with `STATE_CHAIN_MISMATCH` before broadcasting.

## 6. Trace

```bash
labcoat trace <txid> --wait --json
```

Returns decoded events for every protostone in the tx (`create`,
`invoke`, `return`, per-protostone vouts computed automatically).

## MCP mode

`labcoat mcp serve` exposes the installed capability set as MCP tools over
stdio. Prefer it when a host supports MCP; the JSON envelopes above are the
fallback.

<!-- BEGIN GENERATED MCP TOOLS -->
- `network_up` — Boot Labcoat Network using the exact runtime bundle for this CLI release. Returns service status and the endpoint manifest.
- `network_down` — Stop all Labcoat Network services.
- `network_status` — Labcoat Network service health, block height, and mempool size.
- `network_mine` — Mine blocks on Labcoat Network.
- `network_fund` — Send BTC from the Labcoat Network faucet wallet to an address.
- `network_reset` — Stop services and wipe all Labcoat Network chain data.
- `network_logs` — Recent Labcoat Network service logs.
- `wallet_init` — Create or load the project wallet keystore. Optional mnemonic (else generated). Generated mnemonics are redacted from the response unless showMnemonic is true.
- `wallet_addresses` — Wallet receive addresses per script type.
- `wallet_utxos` — Spendable wallet UTXOs.
- `build` — Build Cargo contract packages and extract their Wasm-exported ABIs.
- `test` — Build every contract for WASIp1 and run host integration tests; the first build may take several minutes.
- `abi_fetch` — Fetch ABI metadata from the in-process Alkanes indexer.
- `abi_verify` — Compare a deployed ABI with a locally built contract package.
- `call` — Execute a state-changing contract call and wait for its trace.
- `exchange_plan` — Build an owner-partitioned atomic exchange plan and return its base64 PSBT.
- `exchange_settle` — Validate a buyer-signed PSBT, sign seller inputs, and optionally broadcast. broadcast must be true to transact.
- `simulate` — Simulate a deployed contract against live indexed chain state (no transaction).
- `trace` — Decoded protostone traces for a transaction.
- `balance` — Alkanes token balances held by an address.
- `plan` — Run the project's deploy.rs deployment script and reconcile its steps against the network's run file and chain state; shows pending, complete, and conflicting steps without loading a signer.
- `deploy` — Execute the pending deployment steps deploy.rs declares, or only the step named in step. broadcast is required: true signs and broadcasts, false returns the plan without signing. Refuses while a step is in conflict. With wasm, deploys that raw Wasm artifact outside deploy.rs instead, recorded in labcoat.lock only.
- `import` — Record a contract that already exists on chain as a deploy.rs contract step, after comparing its ABI with the local build. Broadcasts nothing.
- `state_list` — List resources and active instances in this environment's durable deployment state.
- `state_show` — Show one resource's active instance from durable state; history: true includes every earlier instance.
- `state_migrate` — Create version-2 durable state from labcoat.lock, backing up the ledger first. Refuses to overwrite existing state.
- `generate_web` — Write a typed TypeScript browser read client from labcoat.lock and built ABIs (no network access).
<!-- END GENERATED MCP TOOLS -->

## Ground rules

- One Labcoat Network per machine: `up`/`down`/`reset` manage shared local state.
- `reset -y` wipes the chain — deployments in labcoat.lock become stale;
  redeploy after a reset.
- alkanes-rs is pinned (TOOLCHAIN.md). Never point anything at a branch.
- `labcoat docs --llm` prints the full reference document.
