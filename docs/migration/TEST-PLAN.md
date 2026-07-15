# Manual test plan — monorepo migration PR

Covers what CI cannot: real service binaries, a live devnet loop, the
desktop GUI, the extension, MCP against a real host, and the release
workflows. CI already covers builds, unit tests, clippy, the stub devnet
smoke, and the oyl-sdk / alkanes-rs-pin guards — none of that is repeated
here.

Run top to bottom on a normal (unrestricted-network) dev machine.
Estimated time: ~1.5h, mostly waiting on first builds.

## 0. Prerequisites

```bash
git fetch origin claude/labcoat-monorepo-migration-op07m8
git checkout claude/labcoat-monorepo-migration-op07m8
pnpm install && pnpm -r build
cargo build --release -p labcoat-cli        # first build is slow (alkanes-rs graph)
export PATH="$PWD/target/release:$PATH"
labcoat doctor
```

- [ ] `doctor` reports ✓ for cargo, wasm32-unknown-unknown, node
      (warnings about missing binaries/wallet are expected at this point)

## 1. Headless devnet (real binaries)

```bash
labcoat up            # downloads Bitcoin Core, ord, rockshrew-mono, flextrs, espo, jsonrpc bundle
```

- [ ] Downloads complete with checksum verification (watch stderr; only
      ord non-darwin and some service binaries legitimately skip checksums)
- [ ] Ends with "Devnet is up", unified JSON-RPC `http://127.0.0.1:18888`,
      block height ≥ 101 (wallet bootstrap mined coinbase maturity)
- [ ] `labcoat status` → all six services `running`, `ready: true`
- [ ] `labcoat mine 5` → height increases by exactly 5
- [ ] `labcoat logs --service metashrew --limit 20` shows indexer progress
- [ ] Exit the shell that ran `up`, open a new one: `labcoat status`
      still shows everything running (services must survive the CLI)
- [ ] `curl -s http://127.0.0.1:18888` answers (the gateway is up)

Snapshot/restore:

```bash
labcoat snapshot before-test
labcoat up --no-download && labcoat mine 3
labcoat restore before-test && labcoat up --no-download
```

- [ ] After restore, `labcoat status` shows the pre-mine block height

## 2. Contract loop (CLI)

```bash
mkdir /tmp/lab-e2e && cd /tmp/lab-e2e
labcoat wallet init                       # note the warning about the dev passphrase
labcoat wallet addresses                  # copy the p2tr address
labcoat fund <p2tr-address> && labcoat mine 1
labcoat wallet utxos                      # ≥ 1 spendable UTXO
curl -sL https://raw.githubusercontent.com/jonatns/labcoat-templates/main/default/contracts/Example.rs -o Example.rs
labcoat compile Example.rs
labcoat deploy build/Example.wasm --dry-run    # sanity: shows size + sha256, no broadcast
labcoat deploy build/Example.wasm
```

- [ ] Deploy returns `status: success` and an `alkanesId` (`2:N`), and
      `labcoat lock show` contains it under `regtest`
- [ ] `labcoat simulate Example 1 World` → `Hello World!`
- [ ] `labcoat call Example 1 World` → `status: success`, txid present
- [ ] `labcoat trace <that-txid>` shows invoke/return events
- [ ] Revert path: `labcoat simulate Example 99` (bogus opcode) fails
      with a decoded error, not a panic; `--json` variant carries
      `error.code` + `error.hint`

## 3. TS package

In the same `/tmp/lab-e2e` dir:

```bash
cat > labcoat.config.js <<'EOF'
export default { network: "regtest" };
EOF
cat > run.mjs <<'EOF'
import { labcoat } from "<repo>/packages/labcoat/dist/sdk/index.js";
const { account, deploy, simulate, execute } = await labcoat.setup();
console.log(account.taproot.address);
console.log(await deploy("Example"));
console.log(await simulate("Example", "Greet", ["World"]));   // → Hello World!
console.log((await execute("Example", "Greet", ["World"])).executeResult.txId);
EOF
node run.mjs
```

- [ ] Same taproot address as `labcoat wallet addresses` (single keystore)
- [ ] deploy/simulate/execute all succeed; `executeResult.txId` shape intact
- [ ] **Address-parity check (critical for existing users):** put your
      old labcoat project's mnemonic in `labcoat.config.js` — the taproot
      address must match what the oyl-sdk version derived
- [ ] **Legacy migration:** in an old project with
      `deployments/manifest.json`, run `labcoat lock migrate` — entries
      appear in `labcoat.lock` and `simulate <OldName> ...` resolves

## 4. Isomer desktop app (zero-visual-diff)

```bash
pnpm dev:isomer
```

Compare against the pre-migration app (built from `isomer@pre-monorepo`)
side-by-side if possible:

- [ ] Setup screen → binary download flow with progress
- [ ] Dashboard: service matrix all green, block height ticks on mine
- [ ] Mining panel, faucet panel, logs panel (filter + clear) work
- [ ] Explorer panel: block carousel + block details load
- [ ] Contracts panel lists the Example alkane deployed in §2
- [ ] Wallets panel lists `~/.alkanes` wallets (needs `alkanes-cli` on
      PATH — unchanged behavior), fund + confirm works
- [ ] Settings: change a port, save, restart services — status reflects it
      (ServiceInfo ports now come from config; used to always show defaults)
- [ ] Quit the app → all services stop (Drop semantics unchanged)
- [ ] No visual differences vs the old app

## 5. Extension (excluded from CI on purpose)

```bash
cd apps/isomer-extension && pnpm install && pnpm build
```

- [ ] Builds against the real `pkg.alkanes.build` tarball
- [ ] Load `dist/` unpacked in Chrome; popup connects to the devnet
      gateway on :18888

## 6. Agent surface

- [ ] `labcoat docs --llm` renders sensibly (pipe to a pager)
- [ ] Register the MCP server with a real host, e.g.
      `claude mcp add labcoat -- labcoat mcp serve`, then in a session:
      list tools, call `devnet_status`, `simulate` the Example contract —
      results match the CLI
- [ ] `labcoat call Example 1 World --dry-run --json` shows the exact
      protostone spec and broadcasts nothing (height unchanged)

## 7. Release workflows (no publishing)

- [ ] Actions → "Release (Isomer app + labcoat CLI)" → run with
      `kind=cli` on this branch — draft release gets 4 `labcoat-*`
      binaries + sha256s; delete the draft afterwards
- [ ] Same workflow with `kind=app` — Tauri bundles build on all four
      matrix targets (draft; delete afterwards)
- [ ] "Build and Release Binaries" via dispatch (only if you want to
      pre-stage the first monorepo `binaries-v*` release — remember the
      `binary_manager.rs` URL bump comes after, per docs/RELEASING.md)
- [ ] Confirm `NPM_TOKEN` secret exists before merging (changesets flow
      publishes from `main`)

## 8. Teardown

```bash
labcoat reset -y && labcoat status    # everything stopped, height 0 on next up
```

- [ ] `labcoat doctor` again: ports free, binaries installed

## Sign-off

| Suite | Result | Notes |
|---|---|---|
| 1. Headless devnet | | |
| 2. Contract loop (CLI) | | |
| 3. TS package + migration | | |
| 4. Desktop app | | |
| 5. Extension | | |
| 6. Agent surface | | |
| 7. Release workflows | | |

Known-acceptable gaps: Windows CLI binaries aren't built (app bundles
only); `labcoat logs` timestamps are 0 for file-backed entries (services
print their own); the Wallets panel still shells out to a system
`alkanes-cli` exactly as before the migration.
