# Generated web artifacts

Status: implemented for reads, wallet-signed contract calls, and the
exchange request builder (`labcoat generate web`). Unsigned transaction-plan
builders beyond the request file are future work, as are deploys, plain
Alkane transfers, nested SegWit inputs, and a key-holding signer for public
networks.

## Decision

`labcoat generate web` emits a deterministic, self-contained TypeScript module
tree that browser applications import to read a Labcoat-deployed project and
send calls to it. The
output is *derived code* over verified local state — the same posture as
`labcoat.lock` in the durable-state plan: the lockfile is the per-network
ledger of verified deployments, and generated artifacts are a projection of
that ledger plus built ABI metadata. Generation therefore requires **no
network access** and is safe to run in CI.

`generate` is preferred over `export`: the output is typed code and metadata
(manifest, ABI descriptors, a read client), not a copy of the lock file.

## Inputs

- `labcoat.lock` — deployment records for the resolved network target
  (contract name, alkanes id, txid, wasm sha256, chain instance id);
- `build/*.abi.json` — ABI documents written by `labcoat build` (the same
  canonical `__meta` model the CLI uses for named calls);
- resolved settings — network target and unified JSON-RPC endpoint from
  `labcoat.toml` / `LABCOAT_*` / flags.

## Output

`labcoat generate web --out-dir <dir>` writes the tree below. Left out,
`--out-dir` is `generated/web` and `--build-dir` is `build`, both in the
project root from any directory inside the project. A path that is passed is
relative to the working directory, like every other path typed on the command
line. The result reports both directories as absolute paths (`outDir`,
`buildDir`).

- `manifest.ts` — network target, Bitcoin network, RPC endpoint, the chain
  instance id when every record agrees, and one record per lockfile contract.
  Each record links to the local build artifact whose ABI describes it,
  matched by wasm sha256 first and artifact name second.
  The records are also exported as `CONTRACTS`, an `as const` object, so
  contract names and their artifacts are literal types.
- `abi/document.ts`, `abi/<artifact>.ts`, `abi/index.ts` — typed method
  descriptors (opcode as `bigint`, parameter and return type names) derived
  from the existing ABI model, one module per build artifact. Each is
  `as const satisfies AbiDocument`: the client's types are computed from
  these literals, so no per-contract client code is generated.
- `client.ts` — a dependency-free, fetch-based client, made with
  `createClient({ rpc })`:
  - `contract(name)` and `contract(id, abi)` — a handle on one contract.
    `read.<method>(...args)` simulates the call (`secondaryview` →
    `simulate` over a cellpack-carrying `MessageContextParcel`) and returns
    the value decoded per the ABI: `u128` as `bigint`, ids as canonical
    decimal strings, `Vec<u8>` as `Uint8Array`. A revert throws
    `ContractRevertError`; `tryRead.<method>` returns it. Method names,
    argument types, and return types are derived from the ABI at the type
    level, with method names in camelCase. Methods whose camelCase names
    collide keep their exact ABI names;
  - `decodeU128(bytes, offset?)`, `decodeAlkaneId(bytes, offset?)`, and
    `decodeAlkaneIds(bytes)` — decoders for what contracts pack into a
    `Vec<u8>`: a little-endian `u128`, an Alkane id (block then tx, 32
    bytes), and a list of ids. Ids are `{ block, tx }` with decimal strings,
    the shape a read returns for an `AlkaneId`, from the same decoder. Data
    that does not fit throws `RESPONSE_INVALID`;
  - `at(height)` — a client sharing the same connection whose contract
    reads are pinned to one indexed height;
  - `getIndexedHeight()` — the Alkanes secondary-index height
    (`secondaryheight`);
  - `getAlkanesBalances(address)` — full-u128 balances summed across the
    address's outpoints (`secondaryview` → `protorunesbyaddress`);
  - `simulate(contract, method, args)` — the untyped call underneath
    `read`, by ABI method name or decimal opcode, returning the raw bytes
    and `status` without throwing on a revert.

  Calls made in the same tick are sent as one JSON-RPC batch request, and
  unpinned reads that start together share one height lookup.

  Writes:
  - `contract(...).write.<method>(...args, options)` — simulates the call,
    lists the wallet's outputs (`secondaryview` → `esplora` →
    `utxosbyscripthash`, and `protorunesbyaddress` for what each carries),
    builds the unsigned transaction, has the wallet sign it, checks that the
    signed transaction is the one it built, and broadcasts it
    (`sendrawtransaction`). Options: `wallet`, `alkanes`, `pay`, `to`,
    `feeRate`, `simulate`. It resolves to `{ txid, fee, payments, wait() }`;
  - `connect(options)` — the dev wallet on Labcoat Network, else an
    installed browser wallet; `availableWallets()` lists them;
  - `waitForTransaction(txid)` and `wait()` — poll the `trace` view at the
    call's virtual output until the index has it, and return the return
    data or throw `ContractRevertError`.
- `transaction.ts` — the transaction builder, with no network access:
  address decoding, input selection, the protostone, fee and change, the
  PSBT (`witnessUtxo`, and `tapInternalKey` on taproot inputs), and turning
  a signed PSBT into a transaction. It mirrors the Rust call path
  (`toolkit::call`, `execute::spec_with_options`, the pinned upstream
  executor) rule for rule: outputs that carry Alkanes are spent only when
  they hold one the call sends in; Alkanes sent in go through a first
  protostone that forwards the exact amounts; fees are computed in the same
  single-precision arithmetic.
- `wallet.ts` — the `LabcoatWallet` interface (`name`, `getAccounts`,
  `signPsbt`, optional `afterBroadcast`), `devWallet` over
  `labcoat_dev_addresses` and `labcoat_dev_sign_psbt`, `approvalPrompt`, and
  adapters over the injected APIs of UniSat, Xverse, OKX Wallet, and
  Leather. The `TransactionSummary` a wallet is handed names the contract by
  id, and by `contractName` when the call went through a manifest name; the
  prompt shows `name (id)`.
- `errors.ts` — `LabcoatClientError`, `ContractRevertError`, and
  `WalletRejectedError`, re-exported by `client.ts`.
- `exchange.ts` — the exchange request builder for the CLI settlement
  ceremony:
  - `serializeExchangeRequest(request)` — validates an
    `AtomicExchangeRequest` (positive u64 amounts, distinct assets and
    addresses) and serializes it to the **version-1 exchange request file**
    that `labcoat exchange-plan --request <file>` consumes. The format is
    pretty JSON with fixed key order and every u64 as a canonical decimal
    string (JavaScript numbers cannot hold u64), byte-identical to
    `AtomicExchangeRequest::to_request_file` on the Rust side;
  - `exchangeCeremonyCommands(paths)` — renders the exact
    `exchange-plan` → `wallet sign-psbt` → `exchange-settle` command
    sequence for a serialized request.

Every file starts with a `// Generated by \`labcoat generate web\` — do not
edit.` header. Regeneration deletes exactly the `.ts` files carrying that
header before writing, so removed contracts do not leave dead modules while
hand-written files in the same directory survive.

## Boundaries

- **No keys.** The client holds no key material: every transaction is signed
  by a wallet behind the `LabcoatWallet` interface. The dev wallet's key
  stays in Labcoat Network, which signs only for pages served from this
  machine. The exchange builder only serializes a *request* — planning
  (UTXO selection, PSBT construction), signing, and settlement all stay in
  the CLI ceremony. Builders for richer unsigned transaction plans remain
  gated on the durable-state transaction-plan work.
- **One builder, two languages.** The Rust builder depends on tokio and the
  upstream provider, so it is not compiled for the browser; the TypeScript
  one is a second implementation, and test vectors keep it honest (see
  Verification). A change to how `labcoat call` builds a transaction must be
  made in both.
- **Who mines.** Labcoat Network mines nothing by itself. The dev wallet
  asks for a block after each broadcast (`afterBroadcast`), because it is
  the one component that exists only there; `waitForTransaction` never
  mines, so it does the same thing on every network.
- **Not in a write.** `--pointer`, `--refund`, `--edict`, and bitcoin
  `--inputs` have no option; results and refunds go to output 0.
- **No application semantics.** Labcoat emits network identity, ids, ABI
  shapes, and transport. What a contract's bytes *mean* stays in the
  application that consumes them.
- **Browser transport is the app's concern.** The manifest records the
  endpoint labcoat resolved at generation time; browsers normally reach it
  through the app's same-origin dev proxy or rewrite (the client takes an
  `rpc` option), so Labcoat's stack needs no CORS configuration.
- **Stale-state detection stays honest.** Records carry their chain instance
  id (the hash of block 1). After a `labcoat reset`, regenerating updates the
  manifest; a client comparing the manifest's `chainId` against live state
  can detect stale artifacts the same way the CLI detects stale lockfile
  records.

## Verification

- Golden-file codegen tests (`crates/labcoat-core/tests/generate_web.rs`)
  regenerate from fixture lock + ABI files and compare byte-exact output;
  bless intentional changes with `LABCOAT_BLESS=1`.
- `scripts/tests/generate-web-client-test.sh` compiles the golden tree
  under a strict browser TypeScript configuration (`strict`,
  `noUncheckedIndexedAccess`, `exactOptionalPropertyTypes`,
  `verbatimModuleSyntax`, bundler resolution) — consumers vendor the output
  into their own compilation unit — together with type-level checks
  (`checks/types.ts`, where a wrong method name or argument must be a
  compile error) and runs runtime checks against a stub `fetch`
  (`checks/runtime.test.ts`). CI runs it in the `guards` job. The output
  needs TypeScript 4.9 or newer.
- Write vectors. `crates/labcoat-core/src/web_write_vectors.rs` runs the
  Rust call path over a fixed wallet (the options and spec `labcoat call`
  produces, Labcoat's protection of Alkane-carrying outputs, the pinned
  upstream executor with its mock wallet) for a set of calls, and records
  each unsigned transaction in `checks/write-vectors.json`; the test fails
  if the Rust path stops building them. `checks/write.test.ts`, run by the
  same script, sends each case through the generated client against a stub
  endpoint and compares the PSBT's transaction byte for byte. The cases
  cover arguments of every type, change kept and dropped, several inputs,
  fractional and high fee rates, another recipient, payments to every
  address type, and Alkanes sent in from one and several outputs. Not
  covered: a cellpack of 48 bytes or more (the upstream mock wallet cannot
  build one), and non-taproot inputs, which the Rust wallet never has.
  The same file tests the dev wallet, the wallet adapters against stub
  providers, rejection, and waiting.
- `scripts/tests/generate-web-write-e2e.sh` (not run in CI, which has no
  Alkanes indexer): starts a Labcoat Network of its own, deploys the counter
  template, generates the client, and from Node sends writes with the dev
  wallet, waits for them, reads the new value back, sends one that reverts,
  rejects one, and leaves one pending.
- Browser wallets are checked by hand on signet; the checklist is in the
  Web apps guide.
- Manual against-live check (not run in CI): with a project's stack up and
  seeded (`labcoat up`, deploys applied), run `labcoat generate web` in the
  project, point the app's dev proxy at the unified endpoint, and confirm
  the client returns the live height, address balances matching
  `labcoat balance`, and simulate results matching `labcoat simulate`.
