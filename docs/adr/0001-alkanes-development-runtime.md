# ADR 0001: An Alkanes development runtime

- Status: proposed
- Date: 2026-11-24

This document records a proposed direction for Labcoat Network and the
evidence behind it. `docs/RELEASING.md` and `TOOLCHAIN.md` describe the
runtime that ships.

What exists so far is `crates/labcoat-runtime`, a first building block that
the CLI does not use: an indexer host that runs `alkanes.wasm` over state in
memory, a regtest block builder with no transaction validation, and
snapshots. Neither step below is done.

## Context

### What Labcoat Network is today

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

### What the current design costs users

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

Labcoat will own its development runtime, in two steps.

### Step 1: remove Esplorashrew

Serve the wallet's UTXO lookups and the trace command's transaction lookups
from the node's own RPC (`scantxoutset`, `getrawtransaction`, `gettxout`)
through an adapter in Labcoat. The runtime bundle becomes `qubitcoind` and
`alkanes.wasm`, both built from public source, and the `prebuilt` entry in
`runtime-sources.json` goes away.

This step keeps the managed-node design. It is small, it removes the
dependency that cannot be rebuilt, and it stands on its own if step 2 is
never taken.

### Step 2: an in-process Alkanes development runtime

Build a Rust library that runs a development chain and the Alkanes indexer
inside the `labcoat` process, and make it the default Labcoat Network.

It has three parts:

1. **A simulated chain.** Blocks, a UTXO set, a mempool, and transaction
   validation including script and signature checks. No peer-to-peer network,
   no proof of work, and no reorganizations unless a test asks for one.
2. **An indexer host.** The same `alkanes.wasm` that production indexers run,
   loaded through a metashrew-compatible host over storage the runtime owns.
3. **A provider.** One in-process interface for the CLI and tests, and the
   same interface served over HTTP by `labcoat up` for generated web clients,
   MCP clients, and other tools.

Rules the design holds to:

- **Never reimplement Alkanes.** Protocol behavior comes from the upstream
  indexer module. Labcoat simulates only the chain underneath it.
- **Keep the public interface.** CLI commands, `labcoat/v1/*` JSON envelopes,
  MCP tools, and the RPC methods that generated web clients call keep working,
  or change through a versioned, documented break.
- **Build everything from pinned public source.** A dependency that cannot be
  rebuilt does not enter the runtime.
- **Instances are isolated and cheap.** A test can create its own chain,
  snapshot it, revert it, and discard it without touching another.

What this is meant to make possible, none of which exists today: no runtime
download, a runtime on every platform the CLI supports, a private chain per
network test, in-memory snapshots, and control over block height and time.

## Options considered

| Option | Summary | Verdict |
|---|---|---|
| A. Keep Qubitcoin and Esplorashrew | The 0.3.0 design, with the reused module | Rejected as a resting state: one module can never be rebuilt |
| B. Qubitcoin with native RPC lookups | Step 1 above | Accepted as step 1 |
| C. Embed Qubitcoin's crates in the CLI | Link the node as a library instead of shipping a binary | Rejected |
| D. Simulated chain plus indexer host | Step 2 above | Accepted as the direction |
| E. Write and own an Esplora indexer module | Replace Esplorashrew with a Labcoat-maintained module | Rejected: option B meets the same need with no new module |

Option C was rejected for three reasons. It moves the RocksDB C++ build, the
one that broke the release, into every CLI build on four targets. Qubitcoin's
crates are not published as a stable library, and its indexer crate depends on
a moving `develop` branch of metashrew, which conflicts with Labcoat's pinning
policy. And a full node does far more than a development chain needs.

## Consequences

Expected benefits:

- The release no longer depends on a private repository, and after step 2 not
  on a C++ toolchain or a per-platform node build.
- Local development needs no download, background process, or port unless
  the developer asks for a served network.
- Each network test can have its own chain.

Costs and risks:

- **Fidelity.** A simulated chain is not Bitcoin Core. Mempool policy, fee and
  relay rules, and standardness limits may differ, and Alkanes deployments
  rely on large commit and reveal transactions where those rules matter. A
  contract flow could pass locally and be rejected on a public network.
- **Ownership.** Labcoat takes on a chain simulator and an indexer host as
  code it maintains, including tracking upstream changes to the host ABI.
- **Client coupling.** The alkanes-rs client library assumes a Qubitcoin or
  Esplora endpoint. The runtime must present what that library expects, or
  Labcoat must supply the data another way.
- **Migration.** Existing chains, snapshots, and recorded deployments on the
  Qubitcoin-backed network do not carry over without a defined path.

Mitigations: keep signet through Subfrost as the check before mainnet, and
run the release acceptance flow against both runtimes while they coexist.

## Open questions

1. Which script verification implementation: a binding to Bitcoin Core's
   consensus library, which reintroduces a C++ build, or a pure-Rust
   implementation such as Qubitcoin's script crate.
2. Which storage backs a served, persistent network. Tests can stay in memory.
3. Whether the alkanes-rs client can take an in-process transport, or needs a
   local HTTP endpoint even for CLI commands.
4. For step 1, whether to prefetch UTXOs into the client's request or to
   translate its `esplora_*` calls locally. `scantxoutset` reads confirmed
   outputs only, so unconfirmed change needs a decision.
5. Whether forking a public network is in scope. The indexer's views do not
   expose raw storage, so it is not a simple read-through.
6. How to align wasmtime versions between `labcoat-test` and the indexer
   host.
7. How durable state's chain identity maps onto per-test instances.
8. Whether the Qubitcoin-backed network stays as an opt-in full-node mode.
9. License review of each upstream crate before it is linked into the CLI.

## Validation before acceptance

- A prototype of step 1 that passes the release acceptance flow with no
  Esplorashrew module loaded.
- A spike of the indexer host that indexes a deploy and a call, and returns
  the same `simulate` and `trace` results as the Qubitcoin-backed network for
  the starter Counter project. `crates/labcoat-runtime` covers the first part:
  its tests run the `alkanes.wasm` published with `cli-v0.3.0` over the genesis
  block and empty blocks and read back the `sequence`, `getbytecode`, and
  `getblock` views. Deploys, calls, and the comparison are still to do.
- A written list of the node behaviors the simulated chain will not model.

## References

- `docs/RELEASING.md` and `TOOLCHAIN.md`: the runtime as shipped, including
  the prebuilt Esplorashrew module and the pinned build runners.
- [NomicFoundation/edr](https://github.com/NomicFoundation/edr) and
  [Simulated Networks by EDR](https://hardhat.org/docs/explanations/edr-simulated-networks).
- [subfrost/qubitcoin](https://github.com/subfrost/qubitcoin),
  [kungfuflex/alkanes-rs](https://github.com/kungfuflex/alkanes-rs), and
  [kungfuflex/metashrew](https://github.com/kungfuflex/metashrew).
