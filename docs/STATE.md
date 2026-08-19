# Durable state (Milestone 1)

Status: the durable-state foundation of `DURABLE-STATE-PLAN.md` Milestone 1
is implemented for local development — version-2 per-environment state, a
locked/atomic/fail-closed backend, explicit v1 migration, the `labcoat
state` commands, deploy-time history recording, and reset detection. It is
a local regtest development foundation, **not** team-, remote-, or
mainnet-ready durable state; the planner, apply engine, and every later
milestone remain proposed.

## The three files

```text
labcoat.lock                              v1 per-network active-address book
.labcoat/state/<network>.json             apply call journal (unchanged)
.labcoat/state/<environment>/state.json   version-2 durable state
.labcoat/state/<environment>/state.lock   the environment lease
.labcoat/state/<environment>/backups/     state.json.prev + migrate backups
```

`labcoat.lock` stays canonical for name resolution and integrations; the
compatibility export regenerates it from durable state, byte-identically
for labcoat-written files. The version-2 file adds what the lockfile
cannot hold: a lineage UUID, a monotonic serial, explicit chain identity,
and an append-only instance history per resource — a redeploy appends
`instance-2` and moves the active pointer instead of erasing `instance-1`.
(The design doc's flat `.labcoat/state/<environment>.json` location became
a per-environment directory because the flat namespace already belongs to
the apply call journal.)

## Environments

The durable-state environment defaults to `default` and is selected by the
usual precedence: `--environment` flag, then `LABCOAT_ENVIRONMENT`, then
`environment = "..."` in `labcoat.toml`. One environment binds to one
chain instance; point a second environment at a second network or chain
instead of re-pointing an existing one.

## Workflow

```sh
labcoat state migrate                 # once per environment
labcoat state list
labcoat state show counter --history
```

`state migrate` copies the resolved network's `labcoat.lock` records into
version-2 state as `imported` instances, taking a timestamped backup of
`labcoat.lock` into the environment's `backups/` directory first and
refusing to run onto existing state. It records the chain identity it can
observe: the hash of block 1, and — on the managed Labcoat Network — the
persistent instance UUID that `labcoat status` shows.

Once state exists, every `labcoat deploy` (and apply reserve adoption)
appends the new instance to the resource's history under the environment
lease, alongside the unchanged `labcoat.lock` write. Projects that never
run `state migrate` are unaffected — without state the recording is
dormant.

## Crash and concurrency semantics

- State writes are compare-and-swap on the serial, temp-file + fsync +
  atomic rename, with the previous state kept as `backups/state.json.prev`.
  A crash mid-write leaves the old or the new file, never a partial one.
- A corrupt or truncated state file is an error (`STATE_INVALID`), never an
  empty ledger; an unknown schema version is `STATE_UNSUPPORTED`.
- Mutating operations hold an exclusive OS lease on `state.lock`
  (`STATE_LOCKED` on contention). The kernel releases a crashed holder's
  lease automatically — never delete `state.lock` while a labcoat process
  runs.

## Chain identity and reset

State records the network, the hash of block 1 (block 0 is identical
across regtest resets), and the Labcoat Network instance UUID. The UUID
lives inside the chain data directory, so `labcoat reset` regenerates it.
Before any recording, the observed identity is validated against the
recorded one: after a reset, the next deploy fails with
`STATE_CHAIN_MISMATCH` *before broadcasting*, instead of silently mixing
two chains' histories. Archive the environment directory (or use a fresh
`--environment`) to continue on the new chain.

## Verification

- Unit and crash-simulation tests live in `crates/labcoat-core/src/state.rs`
  and `state_backend.rs`, including a two-process lease-contention test.
- The golden migration test
  (`crates/labcoat-core/tests/state_migrate.rs`) pins the migrated state
  bytes and the byte-identical lockfile round-trip; bless intentional
  schema changes with `LABCOAT_BLESS=1`.
- Manual against-live check (not run in CI): in a project with deploys,
  run `labcoat state migrate`, `labcoat state list`, deploy again and see
  `instance-2` in `state show <name> --history`, then `labcoat reset -y`,
  `labcoat up`, and confirm the next deploy fails with
  `STATE_CHAIN_MISMATCH` before broadcasting.
