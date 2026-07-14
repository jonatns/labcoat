# oyl-sdk audit (Phase 0)

Hard constraint: `oyl-sdk` / `@oyl/sdk` is banned from the dependency
tree, direct or transitive. This was the removal ledger; **Phase 3
eliminated every entry below** (along with the `alkanes` npm dep, whose
protostone encoding moved into the Rust core). CI enforces the ban
strictly on every PR: `pnpm ls -r`, lockfiles, and `Cargo.lock` are all
grepped. Kept for historical reference.

## Direct dependency

- `package.json` (toolkit): `"@oyl/sdk": "git+https://github.com/jonatns/oyl-sdk.git"`
  → resolves to `@oyl/sdk@1.18.1` (fork, commit `9f5fceee…`).

## Import surface (6 files, all in `src/sdk/`)

| File | Symbols used | Replacement (Phase 3) |
|---|---|---|
| `wallet.ts` | `Network`, `new Provider({version,url,projectId,network,networkType})` | `alkanes_cli_sys::SystemAlkanes` / `ConcreteProvider` (rust core) |
| `account.ts` | `mnemonicToAccount`, `getWalletPrivateKeys`, `new Signer(...)` | `alkanes_cli_common::keystore::Keystore` (same BIP-86/84/49/44 paths ⇒ same addresses) |
| `runtime.ts` | `utxo.accountUtxos({account, provider})` | executor-internal UTXO selection + `wallet-utxos` envelope command |
| `helpers.ts` | `Provider` type; `provider.alkanes.trace({txid, vout: 4})` | `trace_protostones(txid)` (also fixes the hardcoded vout=4 bug) |
| `execute.ts` | `alkanes.execute({...})`; types `Account/Provider/Signer/FormattedUtxo` | `EnhancedAlkanesExecutor::execute_full` |
| `deploy.ts` | `inscribePayload` (deep import `@oyl/sdk/lib/alkanes/token.js`); same types | executor commit/reveal envelope path (`envelope_data`) |

Plus prose reference in `Readme.md`.

## Transitive findings (from `npm ls`, 2026-07-14)

- `bitcoinjs-lib@6.1.7` currently resolves **only through @oyl/sdk** —
  `wallet.ts` imports it undeclared. Phase 3 either removes the import or
  adds an explicit dependency.
- `alkanes` (kungfuflex, npm) does **not** depend on `@oyl/sdk` — clean.
  (It's removed in Phase 3 regardless; protostone encoding moves into the
  Rust core.)
- `@oyl/sdk` itself pulls `@sadoprotocol/ordit-sdk`, `bip322-js`,
  `alkanes` — all of which leave the tree with it.
- `@alkanes/ts-sdk` (isomer extension) declares `@oyl/sdk` only as an
  **optional peerDependency** → never auto-installed. Allowed, but the CI
  gate watches the resolved tree anyway.

## CI enforcement (Phase 1 onward)

- Fail if `@oyl/sdk` or `oyl-sdk` appears in `pnpm ls -r --depth Infinity`
  output or any lockfile.
- Fail if it appears in `cargo tree` for the workspace.
- Runs on every PR, not once.
