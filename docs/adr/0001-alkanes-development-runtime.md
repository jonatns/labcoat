# ADR 0001: An Alkanes development runtime

- Status: accepted and implemented
- Date: 2026-11-24

Labcoat Network now runs on Labcoat's own runtime, `crates/labcoat-runtime`,
served by the `labcoat` executable. "What shipped" below records how the
implementation differs from the plan. `docs/RELEASING.md`, `TOOLCHAIN.md`, and
the Labcoat Network guide describe the result.

## Context

### What Labcoat Network was

`labcoat up` downloads a runtime bundle published with the CLI release and
starts it as a child process:

| Asset | Source | Role |
|---|---|---|
| `qubitcoind` | `subfrost/qubitcoin` | Bitcoin node in regtest mode; hosts the indexers |
| `alkanes.wasm` | `kungfuflex/alkanes-rs` | The Alkanes indexer: balances, simulation, traces |
| `esplorashrew.wasm` | `kungfuflex/esplorashrew-rs` | Esplora-style index: address UTXOs and transactions |

The CLI talks to that process over JSON-RPC. There is one chain per machine,
shared by every project.

### What the 0.3.0 release showed

The first 0.3.0 publish run failed in three runtime jobs. None was caused by
Labcoat's own code:

- `kungfuflex/esplorashrew-rs` stopped being publicly readable, so the release
  could not build `esplorashrew.wasm`. No public fork or replacement exists,
  and Qubitcoin's own README still points at that repository. 0.3.0 shipped by
  reusing the module that `cli-v0.2.0` built from the same revision.
- GitHub retired the `macos-14` runner image the macOS node build used.
- `ubuntu-latest` moved to Ubuntu 26.04, whose GCC rejects the RocksDB 8.1.1
  that Qubitcoin vendors.

The runtime is the part of the product Labcoat controls least. It depends on
two third-party repositories staying public, on a C++ toolchain, and on a
build that only release day exercises.

### What that design cost users

- A download before the first `labcoat up`, and a node build for only two of
  the four platforms the CLI supports (macOS arm64 and Linux x86_64).
- One shared chain. `labcoat test --e2e` resets it and every network test
  then runs against that same chain, so tests cannot be isolated from one
  another.
- Snapshots are copies of the node's data directory.
- A background process and its ports to manage.

### How Ethereum tooling solves this

Hardhat's network is EDR, the Ethereum Development Runtime: a Rust library
that runs simulated chains inside the tool's own process. Each simulation is
isolated, several can run at once, mining is configurable, and a simulation
can fork a remote chain. It does not run a full node. It runs the real
execution engine over state it owns.

No equivalent was found for Bitcoin. Nigiri, Polar, and similar tools bundle a
regtest node with a separate indexer.

### Facts that make a different design possible

Checked against the pinned upstream revisions on 2026-11-24:

- **Labcoat needs little from the chain.** Its own code calls
  `getblockcount`, `getblock`, `getblockhash`, `getblockchaininfo`,
  `gettxout`, `generatetoaddress`, and `sendrawtransaction`, plus the indexer
  calls `secondaryview` and `secondaryheight`.
- **Labcoat needs two things from Esplorashrew:** address UTXOs for the
  wallet, and transaction hex for `labcoat trace`. Both reach it through the
  alkanes-rs client library, which translates `esplora_*` calls into
  Esplorashrew views.
- **Qubitcoin already answers both natively.** The pinned revision registers
  `scantxoutset` (with `addr`, `raw`, and `rawtr` descriptors) and keeps a
  transaction index behind `getrawtransaction`.
- **The Alkanes indexer is a self-contained module.** The released
  `alkanes.wasm` imports five host functions (`__load_input`, `__host_len`,
  `__get`, `__get_len`, `__flush`) and exports its views (`simulate`, `trace`,
  `protorunesbyaddress`, and others) as functions. Anything that feeds it
  blocks and gives it a key-value store can run it.
- **Hosts for that module exist as libraries.** `metashrew-runtime` is generic
  over its storage adapter, and `memshrew-runtime` provides an in-memory one.
  `labcoat-test` already embeds wasmtime to run contracts natively.

## Decision

Labcoat owns its development runtime. The plan had two steps: first serve
wallet and trace lookups from Qubitcoin's own RPC to drop Esplorashrew, then
replace Qubitcoin. Both were done at once, because the second step removed the
first's reason to exist.

### What shipped

`crates/labcoat-runtime` holds the chain and the Alkanes indexer in one Rust
library:

- **A regtest chain** that starts at Bitcoin's regtest genesis block and
  mines blocks with BIP 34 heights, witness commitments, and valid proof of
  work, paying the subsidy and fees to the requested address.
- **A ledger and mempool**: unspent outputs, a transaction index, spenders, an
  index of outputs by script, and chains of unconfirmed transactions.
- **Validation** following Bitcoin Core's consensus rules and default relay
  policy: signatures and scripts for taproot key-path and tapscript spends,
  P2WPKH, P2PKH, and P2WPKH nested in P2SH; coinbase maturity; absolute and
  BIP 68 relative timelocks against median time past; the 1 sat/vB minimum
  fee; dust; the 400,000 weight-unit limit; conflicts.
  Tapscript runs on a small interpreter covering the opcodes envelopes and
  signature checks use. Unsupported scripts, such as P2WSH, are rejected by
  name rather than accepted unchecked.
- **An indexer host** for metashrew-compatible modules. It runs the same
  `alkanes.wasm` production indexers run, through the module's `env`
  imports, over an in-memory map. A block's writes apply only if the module
  finishes, and views cannot write.
- **Esplora views** answered from the ledger, replacing Esplorashrew, in the
  byte format Esplorashrew used.
- **Persistence**: an append-only file of each block and the indexer writes it
  caused. A restart replays the writes without running WebAssembly; a
  different module makes the runtime index the blocks again. A block is
  saved before it is committed in memory, so a failed write leaves the node
  where it was.
- **A compiled-module cache**, so only the first start of a release compiles
  `alkanes.wasm`.
- **A JSON-RPC server** with the subset of Bitcoin Core and Qubitcoin methods
  Labcoat and the Alkanes client library call, found by recording their
  traffic against Qubitcoin, plus `labcoat_snapshot` and `labcoat_revert`.

`labcoat up` runs that server as a background `labcoat node` process. The
plan considered linking the chain into every CLI command instead; a
long-running process is what lets separate commands, generated web clients,
and MCP clients share one chain, and it costs nothing in startup time because
reloading is instant.

The runtime bundle is now one file, `alkanes.wasm`, built from public source.
Releases no longer build `qubitcoind` or touch Esplorashrew, and Labcoat
Network runs on all four platforms the CLI supports.

### Measured result

The release acceptance flow, on the same machine, before and after:

| Step | Qubitcoin | Labcoat runtime |
|---|---|---|
| First `labcoat up` of a release (compiles the indexer) | 26.4 s | 1.9 s |
| `labcoat up` on a new chain | 26.4 s | 0.8 s |
| `labcoat up` resuming a chain | 16.5 s | 0.2 s |
| `labcoat call` | 4.7 s | 0.2 s |
| `labcoat fund` | 5.4 s | 0.07 s |
| `labcoat wallet utxos` | 2.0 s | 0.08 s |
| `labcoat simulate` | 0.18 s | 0.10 s |

`simulate` and `trace` return the same results on both. Part of the `fund`
and `call` gains came from reusing one RPC client per command instead of
building one, with its TLS roots, per request.

## Options considered

| Option | Summary | Verdict |
|---|---|---|
| A. Keep Qubitcoin and Esplorashrew | The 0.3.0 design, with the reused module | Rejected as a resting state: one module can never be rebuilt |
| B. Qubitcoin with native RPC lookups | Serve lookups from Qubitcoin's RPC, drop Esplorashrew | Superseded by D |
| C. Embed Qubitcoin's crates in the CLI | Link the node as a library instead of shipping a binary | Rejected |
| D. Own chain plus indexer host | What shipped | Accepted |
| E. Write and own an Esplora indexer module | Replace Esplorashrew with a Labcoat-maintained module | Rejected: option B meets the same need with no new module |

Option C was rejected for three reasons. It moves the RocksDB C++ build, the
one that broke the release, into every CLI build on four targets. Qubitcoin's
crates are not published as a stable library, and its indexer crate depends on
a moving `develop` branch of metashrew, which conflicts with Labcoat's pinning
policy. And a full node does far more than a development chain needs.

## Consequences

- Releases depend on no private repository, no C++ toolchain, and no
  per-platform node build.
- Labcoat Network runs wherever the CLI does.
- **Fidelity** is now Labcoat's responsibility. Validation covers what wallets
  and Alkanes deployments produce, and refuses scripts it cannot check, but it
  is not Bitcoin Core: package relay, replace-by-fee rules, and other policy
  details are not modelled. Signet through Subfrost remains the check before
  mainnet.
- **Ownership.** Labcoat maintains the chain, validation, and indexer host,
  and follows changes to the metashrew host interface when it upgrades
  `alkanes.wasm`.
- **Migration.** Chains and snapshots from Qubitcoin-based releases are not
  read. Projects keep their deployment records; the next `labcoat deploy`
  replaces them on the new chain.

## Open questions, as resolved

1. **Script verification**: pure Rust. Sighashes come from the `bitcoin`
   crate and signatures from `secp256k1`, both already in the tree; tapscript
   runs on Labcoat's interpreter. No C++ build was added.
2. **Storage**: memory, saved as an append-only log of blocks and index writes.
3. **Transport**: the CLI keeps talking JSON-RPC to a local process, so the
   Alkanes client library needed no change.
4. **UTXO lookups**: answered natively; Esplora views list confirmed outputs,
   as Esplorashrew did.
5. **Forking a public network**: not in scope.
6. **wasmtime**: the runtime and `labcoat-test` both use wasmtime 51.
7. **Chain identity**: the network keeps its instance id beside the chain, so
   `labcoat reset` gives a new chain a new id, as before.
8. **A Qubitcoin full-node mode**: removed.
9. **Licensing**: no upstream runtime crate is linked; the indexer host is
   Labcoat's own code, and `alkanes.wasm` is still downloaded, not linked.

Still open: the mempool is not saved across `labcoat down`, a chain cannot be
reorganized on request, and per-test chains in `labcoat test --e2e` would need
the harness to create them through `labcoat_snapshot`.

## Validation

- The release acceptance flow passes with no Qubitcoin or Esplorashrew: up,
  native tests, deploy, simulate, call, trace, restart, fund, and mine.
- `labcoat test --e2e` on the starter project passes, including its
  multi-wallet test.
- `crates/labcoat-runtime` tests signed spends of every supported script
  type, the relay rules, the interpreter, persistence, and the RPC server over
  HTTP. Its `tests/alkanes.rs` runs the released `alkanes.wasm`.
- A project with deployment records from a Qubitcoin chain deploys again on
  the new chain.

## References

- `docs/RELEASING.md`, `TOOLCHAIN.md`, and the Labcoat Network guide: the
  runtime as shipped.
- [NomicFoundation/edr](https://github.com/NomicFoundation/edr) and
  [Simulated Networks by EDR](https://hardhat.org/docs/explanations/edr-simulated-networks).
- [subfrost/qubitcoin](https://github.com/subfrost/qubitcoin),
  [kungfuflex/alkanes-rs](https://github.com/kungfuflex/alkanes-rs), and
  [kungfuflex/metashrew](https://github.com/kungfuflex/metashrew).
