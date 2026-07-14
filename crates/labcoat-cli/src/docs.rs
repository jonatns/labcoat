//! `labcoat docs --llm` — the whole toolkit as one agent-ready document.

pub fn llm_reference() -> String {
    format!(
        r#"# Labcoat — command reference & protocol cheatsheet (v{version})

Labcoat is a smart-contract development toolkit for Alkanes on Bitcoin.
Isomer is the one-click local devnet inside it (Foundry ⊃ Anvil).

## The core loop

```bash
labcoat up                                  # boot the devnet (bitcoind regtest + indexers + gateway)
labcoat wallet init                         # create the project wallet (.labcoat/wallet.json)
labcoat fund <address> && labcoat mine 1    # give the wallet spendable BTC
labcoat compile contracts/MyToken.rs        # → build/MyToken.{{wasm,wasm.gz,abi.json}}
labcoat deploy build/MyToken.wasm           # commit/reveal deploy; records labcoat.lock
labcoat call MyToken <opcode> [args...]     # state-changing call (auto-mines on regtest)
labcoat simulate MyToken <opcode> [args...] # read-only, no transaction
labcoat trace <txid> --wait                 # decoded protostone traces
labcoat down                                # stop the devnet
```

## JSON envelopes (agent mode)

Every command accepts `--json` and then prints exactly one envelope on
stdout (logs go to stderr; exit code 0 whenever an envelope was emitted):

```json
{{"ok": true,  "command": "deploy", "schema": "labcoat/v1/deploy", "result": {{...}}}}
{{"ok": false, "command": "deploy", "schema": "labcoat/v1/error",
  "error": {{"code": "WALLET_MISSING", "message": "...", "hint": "run `labcoat wallet init` first"}}}}
```

Error codes: CONFIG_INVALID, WALLET_MISSING, WALLET_LOCKED,
RPC_UNREACHABLE, INDEXER_LAG, INSUFFICIENT_FUNDS, EXECUTION_REVERT,
TRACE_TIMEOUT, ENVELOPE_INVALID, COMPILE_FAILED, CONTRACT_NOT_FOUND,
TOOLKIT_ERROR, BINARY_CRASH. The `hint` is always the next command to try.

## Non-interactive conventions

- Secrets never ride argv: passphrase via `LABCOAT_WALLET_PASSPHRASE`
  (regtest falls back to a fixed dev passphrase with a warning; mainnet
  and signet refuse to run without one), mnemonic via `LABCOAT_MNEMONIC`
  or `wallet init --mnemonic-stdin`.
- `labcoat reset -y` skips the confirmation prompt.
- `deploy --dry-run` / `call --dry-run` validate inputs and describe the
  transactions without broadcasting.
- Global flags/envs: `--network`/`LABCOAT_NETWORK` (regtest default),
  `--rpc-url`/`LABCOAT_RPC_URL` (default http://localhost:18888),
  `--wallet-file`/`LABCOAT_WALLET_FILE`, `--fee-rate`.

## Devnet commands

| Command | What it does |
|---|---|
| `up [--no-download]` | fetch missing binaries, boot bitcoind/metashrew/ord/esplora/espo/gateway, bootstrap + fund the dev wallet, emit the endpoint manifest |
| `down` | stop every devnet service (owned or detached) |
| `status` | per-service health + block height + mempool + readiness |
| `mine [count] [--address A]` | mine blocks (max 1000/invocation) |
| `fund <address> [amount]` | faucet BTC from the dev wallet |
| `logs [--service S] [--limit N]` | recent service logs (file-backed) |
| `reset [-y]` | stop + wipe all chain/index data |
| `snapshot [name] [--list]` / `restore <name>` | copy-on-stop devnet state snapshots |
| `binaries [--download]` | check/fetch service binaries |

## Contract commands

| Command | What it does |
|---|---|
| `wallet init [--mnemonic-stdin]` | create/load the keystore (BIP-86/84/49/44; same mnemonic ⇒ same addresses as ever) |
| `wallet addresses [--count N]` | receive addresses per script type |
| `wallet utxos` | spendable UTXOs |
| `compile <file.rs \| dir> [--name N] [--out-dir D]` | cargo → wasm32-unknown-unknown → raw .wasm + .wasm.gz + ABI (regex over `#[opcode(n)]` grammar) |
| `deploy <wasm> [--name N] [--args a,b] [--dry-run]` | commit/reveal envelope deploy of the RAW .wasm (never .wasm.gz — the envelope compresses internally); waits for the create trace; writes labcoat.lock |
| `call <name\|block:tx> <opcode> [args...] [--dry-run]` | execute; waits for indexer sync + trace; decodes revert reasons |
| `simulate <name\|block:tx> <opcode> [args...]` | read-only metashrew simulation; result decoded as printable string, then integer |
| `trace <txid> [--wait]` | decoded traces for every protostone in the tx (vouts auto-computed) |
| `lock migrate` / `lock show` | one-shot legacy-manifest migration; inspect labcoat.lock |
| `mcp serve` | Model Context Protocol server over stdio exposing all of the above as tools |

## Protocol cheatsheet

- **Cellpack**: the message body of an alkanes call — `[block, tx,
  opcode, ...args]` as u128 values. String args ≤16 bytes are packed
  little-endian into a u128.
- **Deploy** targets cellpack `[1, 0]`: "create a new alkane" — the wasm
  rides a taproot witness envelope (BIN protocol) in a commit/reveal pair.
  The new contract id `block:tx` comes from the `create` trace event.
- **Protostone vouts**: traces attach to virtual outputs `tx.output.len()
  + 1 + i` for protostone i; `trace <txid>` computes this for you.
- **Sync**: state-changing ops wait until the alkanes indexer (metashrew)
  height ≥ chain height so fresh UTXOs are introspectable; `INDEXER_LAG`
  means it never caught up.
- **labcoat.lock**: per-network deployment ledger `{{network: {{Contract:
  {{alkanesId, wasmSha256, txid, status, deployedAt}}}}}}` — `call`/`simulate`
  accept the contract name and resolve through it.
- **Endpoints** (regtest defaults): unified JSON-RPC gateway :18888
  (proxies bitcoind :18443, metashrew :8080, ord :8090, esplora
  :50010/:50001, espo :8083/:8081).

## alkanes-rs pin

All alkanes-rs code paths are pinned to commit
`{rev}` on the `develop` branch — see TOOLCHAIN.md before changing
anything about that.
"#,
        version = env!("CARGO_PKG_VERSION"),
        rev = labcoat_core::compile::ALKANES_RS_REV,
    )
}
