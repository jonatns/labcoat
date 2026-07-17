# Durable state and declarative deployment plan

Status: proposed

## Summary

Build an Alkanes-native reconciliation engine in Labcoat. The engine compares
project configuration, prior Labcoat state, and observed Alkanes state; produces
an explicit plan; and applies that plan through a durable, resumable operation
journal.

The architecture must support an eventual mainnet release. Local state and
software-wallet workflows are sufficient for early regtest milestones, but
mainnet support has additional release gates for shared state, signing, review,
spend policy, auditability, and recovery.

This is broader than making `deploy --dry-run` more realistic. Labcoat needs to
model immutable deployments, reserved contracts, factory instances,
cross-contract dependencies, upgradeable proxies, beacons, authorization
capabilities, and chain observation as different resource lifecycles.

The target workflow is:

```text
labcoat.toml          desired resource topology
        +
Labcoat state         prior instances and operation history
        +
Bitcoin/Metashrew     observed state
        |
        v
labcoat plan          create / replace / configure / upgrade / drift
        |
        v
labcoat apply         journaled, ordered, resumable execution
        |
        v
verified state        active IDs, history, transactions, outputs
```

## Goals

- Preserve every deployment and upgrade instead of overwriting the previous
  Alkane ID for a logical name.
- Make repeated application idempotent when desired and observed state match.
- Represent Alkanes deployment kinds and upgrade patterns explicitly.
- Track typed dependencies so plans show downstream replacements and upgrade
  blast radius.
- Detect drift, reorgs, regtest resets, stale indexer state, and missing or
  mismatched bytecode.
- Make commit/reveal deployment and state-changing calls recoverable after a
  process interruption.
- Resolve IDs assigned during apply without predicting the global sequence.
- Preserve the current CLI and MCP JSON-envelope conventions.
- Never place mnemonics, private keys, or wallet passphrases in state.
- Make every mainnet deployment independently auditable and recoverable from
  chain data plus publishable deployment receipts.

## Non-goals

- Mirroring every contract storage key into Labcoat state.
- Treating one-off calls such as transfers or mints as declarative resources.
- Deleting contracts from the chain. Alkanes deployments are immutable.
- Inferring arbitrary hardcoded cross-contract IDs from Wasm.
- Claiming that a plan simulates a new deployment. Accurate new-Wasm execution
  requires a Metashrew deployment view or an embedded snapshot-capable Alkanes
  runtime.
- Shipping a remote state backend in the first durable-state milestone. The
  backend API is part of that milestone; at least one production backend is a
  requirement before Labcoat is declared mainnet-ready.
- Building a module/chart registry. Reusable deployment modules can be layered
  on the resource model after its lifecycle semantics are stable.

## Current state and gaps

The existing `labcoat.lock` is a version 1 per-network map from logical contract
name to one deployment record. It has several correctness gaps for use as
durable state:

- Recording a deployment with the same name replaces the earlier record.
- Loading silently returns empty state when the file is missing, unreadable, or
  malformed. Corruption can therefore look like a fresh project and cause
  accidental redeployment.
- Saving writes the destination directly, without a temporary file, fsync,
  atomic rename, backup, or process lock.
- Network name is the only environment discriminator. It cannot distinguish
  two regtest instances or detect a reset by itself.
- There is no operation journal, state serial, lineage, or concurrency check.
- The deploy path supports ordinary sequential `CREATE` only.
- Deploy with constructor arguments constructs a `[1,0,0,...]` cellpack and
  therefore assumes initializer opcode `0`. Standard upgradeable Alkanes use
  other initializer opcodes.
- Constructor arguments are untyped raw `u128` values even though local ABI
  metadata contains methods, opcodes, and parameter types.
- `simulate` executes an existing deployed contract through Metashrew; the
  current deploy dry-run only validates local input and prints intent.
- The executor calls upstream `execute_full` as one operation, so Labcoat cannot
  durably checkpoint each commit/reveal phase.

## Architectural decisions

### Three distinct forms of state

1. **Desired state** is human-authored and committed in `labcoat.toml`.
2. **Operational state** is machine-owned and records resource instances,
   relationships, outputs, and every apply operation.
3. **Observed state** is refreshed from Bitcoin and Metashrew. Bitcoin is the
   canonical chain; Metashrew is the indexed Alkanes read model.

Planning is a three-way comparison:

```text
desired configuration <-> prior operational state <-> observed chain state
```

### State location and compatibility

- Store canonical local state at `.labcoat/state/<environment>.json`.
- Add an `environment` setting, defaulting to `default`, independently of
  `network`. For example, `dev` and `staging` can both use regtest or signet
  without sharing state.
- Keep `.labcoat/` ignored by generated projects. Team and production use will
  use a locking, versioned remote backend rather than committing mutable
  operational state.
- Continue reading `labcoat.lock` during migration. Generate it as a
  compatibility address-book containing active IDs only, so existing name
  resolution and integrations keep working.
- Treat operational state as canonical after migration. `labcoat.lock` must
  never be used to reconstruct history unless explicitly imported as legacy
  state.
- Keep the existing guidance that teams may commit `labcoat.lock` when they
  want to publish active deployment addresses. It contains no operation
  journal or secrets.
- Treat operational state as sensitive metadata even though it contains no key
  material. It may reveal wallet addresses, outpoints, deployment timing, and
  operational history, so backends need access control and encryption in
  transit.

### Resource identity

A logical resource address, such as `contract.token_impl`, remains stable while
its on-chain instances accumulate. Each instance has a permanent Alkane ID and
terminal lifecycle status. Replacing a resource creates a new active instance
and marks the old instance `superseded`; it never erases the old record.

### Typed dependency edges

References in desired configuration create graph edges with lifecycle meaning:

- `constructor`: the resolved ID is immutable; a changed dependency replaces
  the consumer.
- `mutable`: a changed value emits a configured setter call with readback.
- `proxy_implementation`: deploy new logic and update one stable proxy.
- `beacon_implementation`: update one beacon and report all affected proxies.
- `beacon`: the beacon proxy remains stable when the beacon implementation
  changes.
- `observed`: report drift but do not mutate the dependency.
- `explicit`: ordering-only dependency for hardcoded IDs or application-level
  constraints that cannot be inferred.

References that appear in initializer arguments default to `constructor`.
Standard proxy and beacon resource drivers provide their edge type. Other
hardcoded or runtime-discovered relationships require `depends_on` or explicit
link metadata.

## Resource model

The first complete resource model should include:

| Resource | On-chain identity | Change behavior |
| --- | --- | --- |
| `contract` with `create` | sequential Alkane ID | deploy replacement |
| `reserved_contract` | fixed reserved ID | create once; mismatch is blocked |
| `factory_instance` | new ID pointing to factory code | deploy replacement |
| `external_alkane` | existing or precompiled ID | observe/import only |
| `upgradeable_proxy` | stable proxy ID | update implementation pointer |
| `upgradeable_beacon` | stable beacon ID | update implementation pointer |
| `beacon_proxy` | stable proxy and beacon IDs | follows beacon; no proxy update |

Factory instances must record both the instance ID and factory ID. Their code
identity is the referenced factory plus the observed factory bytecode hash,
while storage and balances remain instance-specific.

System and virtual precompiles are data sources, not managed deployments.

## Desired configuration

Extend `labcoat.toml` with resources while preserving the existing settings:

```toml
network = "regtest"
environment = "dev"
rpc_url = "http://localhost:18888"
wallet_file = ".labcoat/wallet.json"
fee_rate = 2.0

[resources.token_impl]
kind = "contract"
artifact = "contracts/token"
deployment = "create"
initializer = { method = "initialize", args = ["My Token", "MTK"] }

[resources.token_beacon]
kind = "upgradeable_beacon"
implementation = "${resources.token_impl.id}"
auth_units = 1

[resources.token]
kind = "beacon_proxy"
beacon = "${resources.token_beacon.id}"

[resources.market]
kind = "contract"
artifact = "contracts/market"
deployment = "create"
initializer = { method = "initialize", args = ["${resources.token.id}"] }

[data.frbtc]
kind = "external_alkane"
id = "32:0"
```

The manifest parser must reject unknown fields, duplicate resource addresses,
invalid references, dependency cycles, incompatible deployment options, and
initializer methods or values that do not match the extracted ABI.

An Alkane ID reference is ABI-encoded as the protocol's `(block, tx)` pair. An
ID assigned by sequential create is an unknown plan value and is resolved only
after its deployment has been observed.

## State format

Use schema-versioned JSON for operational state. The exact Rust types are part
of milestone 1, but the persisted shape must cover the following information:

```json
{
  "version": 2,
  "lineage": "019...",
  "serial": 14,
  "environment": "dev",
  "chain": {
    "network": "regtest",
    "genesisHash": "...",
    "devnetInstanceId": "...",
    "lastObservedTip": { "height": 105, "hash": "..." }
  },
  "resources": {
    "contract.token_impl": {
      "kind": "contract",
      "desiredDigest": "...",
      "activeInstance": "instance-2",
      "dependencies": [],
      "instances": [
        {
          "instanceId": "instance-1",
          "alkanesId": "2:7",
          "artifactSha256": "...",
          "abiSha256": "...",
          "labcoatVersion": "...",
          "alkanesRevision": "...",
          "sourceRevision": "...",
          "initializer": { "method": "initialize", "args": [] },
          "commitTxid": "...",
          "revealTxid": "...",
          "blockHeight": 103,
          "blockHash": "...",
          "status": "superseded"
        }
      ]
    }
  },
  "operations": [
    {
      "operationId": "019...",
      "resource": "contract.token_impl",
      "action": "create",
      "status": "verified",
      "transitions": []
    }
  ]
}
```

Required top-level durability fields:

- `version`: persisted schema version.
- `lineage`: UUID that survives serial increments and changes only when state
  is intentionally re-created.
- `serial`: monotonic revision used by plans and compare-and-swap writes.
- `environment`: logical deployment environment.
- `chain`: chain identity and last observed anchor.
- `resources`: logical resources, dependency edges, active pointers, outputs,
  and append-only instance history.
- `operations`: resumable apply journal and transaction transitions.

State must not contain secrets. Wallet addresses, authorization Alkane IDs,
outpoints, and transaction IDs are allowed.

### Operation states

Deploy operations need durable transitions at least at:

```text
prepared
commit_broadcast
commit_confirmed
reveal_broadcast
reveal_confirmed
indexed
verified
```

State-changing calls need `prepared`, `broadcast`, `confirmed`, `indexed`, and
`verified`. Terminal error states include `reverted`, `failed`, `orphaned`, and
`abandoned`.

Persist each transition before beginning the next irreversible step. On
resume, query the mempool, chain, and Metashrew by recorded transaction ID
before deciding whether to retry. Never blindly repeat a pending deployment.

## Durable local backend

Introduce a `StateBackend` abstraction with operations equivalent to:

```rust
trait StateBackend {
    fn lock(&self, environment: &str) -> Result<StateLease>;
    fn load(&self, environment: &str) -> Result<Option<State>>;
    fn compare_and_swap(&self, expected_serial: u64, state: &State) -> Result<()>;
}
```

The local implementation must:

- Take an exclusive per-environment process lock for every mutating command.
- Fail closed on unreadable, malformed, or unsupported state. It must never
  convert corruption into empty state.
- Validate schema and chain identity before planning or applying.
- Write a uniquely named temporary file in the destination directory.
- Flush and fsync the file, atomically rename it, and fsync the parent
  directory where supported.
- Retain a last-known-good backup before migrations.
- Increment `serial` exactly once per successful state transition.
- Reject stale compare-and-swap writes.

The interface should be asynchronous only if a future remote backend requires
it; do not make the local implementation asynchronous without need.

## Mainnet safety model

Mainnet readiness is a release gate, not merely acceptance of
`--network mainnet`. Before Labcoat advertises durable mainnet deployment, it
must meet all requirements in this section.

### Protected environments

Mainnet apply requires an explicitly protected environment. It must not inherit
unsafe defaults from regtest. Configuration needs explicit values for:

- state backend;
- signer;
- confirmation policy;
- maximum fee rate and maximum total fee;
- maximum total BTC spend and expected change policy;
- plan expiry or maximum observed-tip drift;
- upgrade safety and approval policy.

Direct `labcoat deploy` on mainnet must create a saved plan and follow the same
review path as `labcoat apply`. It must not combine planning and broadcasting
behind one confirmation prompt.

### Remote state

Implement and support at least one production state backend before declaring
mainnet readiness. A production backend must provide:

- exclusive leases with expiry and owner identity;
- compare-and-swap by lineage and serial;
- encryption in transit and access control;
- durable version history and point-in-time recovery;
- audit records for lock acquisition and state writes;
- recovery when a client dies while holding a lease.

Local state remains useful for individual experimentation. Mainnet apply with a
local backend should be unsupported by default; any temporary expert override
must be explicit, noisy, excluded from automation, and documented as lacking
team-safety guarantees.

### Signing and custody

Separate planning and transaction construction from signing and broadcast.
Mainnet support requires an external signer or PSBT-compatible flow so users do
not need to expose production mnemonics to Labcoat. The signer interface must:

- identify the expected wallet fingerprint before approval;
- present commit, reveal, fees, outputs, change, and transferred Alkanes for
  review;
- verify that signed transactions match the approved plan;
- support resuming commit/reveal without requesting a second unrelated spend;
- never persist signer credentials in state, plans, receipts, or logs.

An encrypted software wallet may remain available, but environment-variable
mnemonics and the regtest development passphrase are not mainnet deployment
workflows.

Authorization tokens and other Alkanes capabilities require the same custody
discipline as BTC inputs. Planning must identify which capability is needed,
where it is observed, and how the apply will preserve or transfer it.

### Mandatory reviewed plans

A mainnet plan must be saved, content-addressed, and explicitly approved by its
digest. It includes:

- network, genesis hash, environment, and state lineage/serial;
- source revision, Labcoat version, pinned Alkanes revision, artifact hash, and
  ABI hash;
- every transaction intent, BTC input/output policy, fee bound, and Alkanes
  transfer;
- dependency replacements, proxy/beacon blast radius, and unsafe upgrade
  findings;
- required signer and authorization capabilities;
- verification checks and rollback target where one exists.

Apply must rebuild or re-read artifacts and prove their hashes match the plan.
Any changed protected value invalidates approval and requires a new plan.

### Deployment receipts

After verification, emit one immutable, publishable receipt per deployment or
upgrade. A receipt contains no secrets and includes:

- plan digest and resource address;
- chain identity and environment;
- source revision and tool/runtime versions;
- artifact and ABI hashes;
- resolved initializer or upgrade target;
- commit/reveal or call transaction IDs;
- Alkane ID, confirmation block, and verification result;
- previous active instance or implementation when applicable.

Receipts are content-addressed and may be committed, attached to releases, or
stored alongside remote state. `labcoat.lock` is regenerated from verified
state and receipts as the concise active-address view.

### Recovery without trusted local state

Loss of a workstation must not force redeployment or make an upgradeable system
unmanageable. Provide a recovery workflow that accepts `labcoat.lock`, receipts,
and explicit Alkane IDs, then verifies all recoverable facts through Bitcoin
and Metashrew before constructing new operational state.

Recovery cannot reconstruct secrets or undeclared intent. It must label fields
as observed, receipt-proven, or unknown rather than inventing configuration.

## Chain identity and drift

Network name is insufficient state identity. Record and validate:

- normalized network;
- genesis hash;
- a Labcoat-generated devnet instance UUID for managed local devnets;
- deployment transaction and block hashes;
- the last observed tip as a reorg anchor.

Every `labcoat reset` must generate a new devnet instance UUID. Remote regtest
instances without a Labcoat UUID are distinguished by verifying all recorded
deployment transactions and block hashes during refresh.

Refresh should use Bitcoin RPC for transaction inclusion, block hashes, and
confirmations, and Metashrew for:

- indexer height;
- Alkane ID to creation outpoint;
- deployed bytecode and its hash;
- standard storage pointers or view methods;
- traces and execution status;
- ABI metadata where verification is requested.

Drift classifications include:

- `pending`: transaction exists but is not sufficiently confirmed or indexed;
- `orphaned`: recorded transaction or block is no longer canonical;
- `missing`: no observed Alkane exists at the recorded ID;
- `bytecode_mismatch`: observed code differs from state;
- `configuration_drift`: a managed proxy/beacon pointer differs;
- `indexer_stale`: Metashrew cannot yet make a reliable comparison;
- `chain_mismatch`: state belongs to a different chain or devnet instance.

`indexer_stale` must block mutation rather than appear as drift to repair.

## Planning

Plans are immutable artifacts containing:

- state lineage and serial;
- environment and chain identity;
- desired configuration digest;
- artifact and ABI hashes;
- observed tip anchor;
- ordered actions and dependency reasons;
- unknown values that will be resolved after earlier actions;
- estimated fee bounds where available;
- required wallet and authorization capabilities;
- transaction input/output policy and total spend bounds;
- warnings and blocked changes.

Supported actions:

- `create`
- `replace`
- `configure`
- `upgrade`
- `import`
- `forget` (state only; never chain deletion)
- `no_change`
- `drift`
- `blocked`

Examples:

```text
+ token_impl        create (Alkane ID known after apply)
~ token_beacon      upgrade implementation 2:7 -> <token_impl.id>
! token_proxy_a     behavior changes through token_beacon
! token_proxy_b     behavior changes through token_beacon
= market            unchanged; it references a stable proxy ID
```

```text
-/+ token           replace (artifact hash changed)
-/+ market          replace (constructor dependency token changed)
```

Applying a saved plan must fail if state serial, lineage, chain identity,
desired digest, artifact hashes, or protected observed values changed. The user
must create a new plan rather than apply stale intent.

## Applying a plan

Apply actions in topological order:

1. Lock and reload state.
2. Validate the saved plan against current state and chain identity.
3. Recheck Metashrew synchronization, wallet balance, spendable UTXOs, fee
   bounds, total spend policy, signer identity, and authorization capabilities.
4. Write the operation's `prepared` journal entry.
5. Execute and persist each transaction transition.
6. Wait for the configured confirmation policy and Metashrew indexing.
7. Verify bytecode, traces, IDs, and managed configuration readbacks.
8. Append the new instance or mutation to resource history.
9. Move the logical resource's active pointer only after verification.
10. Regenerate the `labcoat.lock` active-address compatibility view.

An apply is not globally atomic. If a later resource fails, completed chain
actions remain completed and recorded. A subsequent apply resumes or produces
a plan from that partial state.

Refactor the current monolithic `execute_full` integration so Labcoat can
observe or own commit/reveal boundaries. Acceptable implementations are:

- a phased executor API that prepares and broadcasts commit and reveal
  separately; or
- an upstream executor event sink that emits transaction IDs before advancing
  to the next irreversible phase.

Do not claim crash-safe deployment until Labcoat can persist the commit txid
before the reveal phase begins.

## Upgradeable resources

Implement standard proxy and beacon support as resource drivers rather than
hardcoded special cases in the planner:

```rust
trait ResourceDriver {
    fn validate(&self, desired: &Resource) -> Result<()>;
    async fn refresh(&self, prior: &ResourceState) -> Result<ObservedState>;
    fn diff(&self, desired: &Resource, observed: &ObservedState) -> Change;
    async fn apply(&self, change: &Change, journal: &mut Journal) -> Result<AppliedState>;
}
```

Initial drivers:

- `ContractDriver`
- `ReservedContractDriver`
- `FactoryInstanceDriver`
- `ExternalAlkaneDriver`
- `UpgradeableProxyDriver`
- `UpgradeableBeaconDriver`
- `BeaconProxyDriver`

Drivers use extracted ABI method names and types rather than embedding opcode
numbers in the planner. Standard implementations may additionally validate
known storage keys and view behavior.

Changing a proxy implementation creates a new implementation deployment and
one pointer update while retaining the proxy ID and storage. Changing a beacon
implementation creates one pointer update and reports every managed beacon
proxy as impacted without modifying those proxies.

Rollback is another forward transaction that points a proxy or beacon to a
previous implementation. It never deletes the failed implementation or erases
the upgrade history.

### Upgrade safety

The current ABI describes methods, opcodes, parameters, and return values but
does not describe delegatecall storage layout. Extend contract metadata with:

- storage schema name and version;
- compatible prior storage versions;
- initializer/reinitializer version;
- declared external links;
- optional upgrade assertions and post-upgrade checks.

Until that metadata exists, proxy and beacon upgrades must be marked unsafe and
require explicit approval. ABI compatibility alone is not sufficient.

Authorization tokens are capabilities. State records which capability is
required and the observed holding/outpoint, while apply verifies that the
selected wallet can exercise it. State never stores key material.

## Simulation and verification

Keep planning, simulation, and verification separate:

- Local plan validation checks Wasm shape, gzip handling, ABI metadata, typed
  arguments, resource references, and dependency cycles.
- Metashrew simulation validates calls and upgrades against existing indexed
  state.
- New deployment simulation remains `not simulated` until Metashrew exposes a
  view that accepts new Wasm/envelope context or Labcoat embeds the exact
  Alkanes runtime against a state snapshot.
- Post-apply verification checks the actual deployed bytecode, trace status,
  Alkane ID, configuration pointers, and declared smoke-test views.

A plan must print `constructor execution: not simulated` when that is true.

## CLI surface

Add:

```text
labcoat plan [--out <file>]
labcoat apply [<plan-file>]
labcoat refresh

labcoat state list
labcoat state show <resource> [--history]
labcoat state migrate
labcoat state import <resource> <block:tx>
labcoat state forget <resource>

labcoat output <resource>.<field>
labcoat rollback <proxy-or-beacon> --to <block:tx|history-selector>
```

Behavioral rules:

- `state forget` changes only Labcoat state and requires confirmation outside
  regtest.
- There is no `destroy` command because deployment deletion is not meaningful.
- Existing `deploy`, `call`, `simulate`, `trace`, and raw `block:tx` addressing
  remain supported.
- Mainnet mutation requires a protected environment, saved approved plan, and
  production state/signer policy; raw convenience commands cannot bypass it.
- Once the apply engine is stable, `labcoat deploy` should execute a synthetic
  one-resource plan so direct deploys receive the same durable journaling.
- Add CLI JSON envelopes first; expose equivalent MCP tools only after command
  schemas and recovery behavior stabilize.

## Code organization

Add the following modules to `labcoat-core`:

```text
manifest.rs          desired resource configuration and references
resource.rs          resource kinds, instances, outputs, and typed edges
state.rs             schema, validation, migration, and state transitions
state_backend.rs     locking and atomic local persistence
observer.rs          Bitcoin/Metashrew refresh and drift classification
graph.rs             dependency graph and cycle detection
plan.rs              three-way diff and immutable plan format
apply.rs             ordered execution and recovery
drivers/             lifecycle-specific resource drivers
```

Refactor existing modules:

- `lockfile.rs`: legacy import plus active-address compatibility export.
- `execute.rs`: generalized deployment targets and phase/event visibility.
- `toolkit.rs`: resource operations and typed invocation entry points.
- `abi.rs`: public parsed ABI model and typed argument encoding.
- `settings.rs`: environment and resource configuration loading.
- `contract.rs` and `main.rs`: new CLI commands and compatibility paths.
- `mcp.rs`: plan/apply/state tools after CLI stabilization.
- `isomer-core`: persistent devnet instance UUID regenerated by reset.

Avoid putting planner or persistence logic in CLI handlers. Both CLI and MCP
must call the same `labcoat-core` operations.

## Milestones

### Milestone 1: durable state foundation

- Define the version 2 state schema, lineage, serial, resources, instances, and
  operations.
- Implement the locked, atomic local backend and fail-closed loading.
- Add explicit v1 `labcoat.lock` migration with a backup.
- Add `state list`, `state show`, `state migrate`, and compatibility export.
- Add devnet instance UUID generation and reset behavior.

Acceptance criteria:

- Killing a state write at any injected point leaves either the old or new
  valid file, never a partial file interpreted as empty.
- Concurrent mutating processes cannot both acquire the environment lease.
- Migrating v1 preserves every available field and creates an `imported`
  instance without inventing missing data.
- A reset devnet is rejected as a chain mismatch before mutation.

### Milestone 2: generalized deployment and ABI invocation

- Replace the hardcoded create cellpack with `DeploymentSpec`.
- Support create, create-reserved, and factory targets.
- Make ABI types and method lookup available to the core toolkit.
- Encode initializer method and arguments from ABI metadata.
- Preserve raw opcode/argument escape hatches for advanced users.

Acceptance criteria:

- A standard initializer whose opcode is not `0` deploys correctly.
- Reserved-ID collision and factory-reference errors are typed and actionable.
- Invalid ABI arguments fail before wallet or network mutation.

### Milestone 3: observation, refresh, and import

- Implement Bitcoin and Metashrew observation.
- Add drift classification and chain identity validation.
- Add `refresh` and `state import`.
- Resolve active resource names from canonical state, with `labcoat.lock`
  fallback during migration.

Acceptance criteria:

- Refresh detects missing transactions, a reorged deployment block, bytecode
  mismatch, stale Metashrew, and a reset regtest.
- Import verifies the Alkane exists and records observed bytecode and outpoint
  information without claiming Labcoat deployed it.

### Milestone 4: graph and planning

- Parse resources and references from `labcoat.toml`.
- Build typed dependency edges and detect cycles.
- Implement three-way diff and saved plan validation.
- Support create, replace, no-change, drift, and blocked actions.
- Show unknown sequential IDs as values resolved after apply.

Acceptance criteria:

- A second plan after a successful unchanged deployment is empty.
- Changing an artifact replaces that resource.
- Changing a constructor-bound dependency replaces its consumers.
- A saved plan is rejected when state serial, artifact, configuration, or chain
  identity changes.

### Milestone 5: journaled and resumable apply

- Refactor deploy execution to expose commit/reveal transitions.
- Implement operation journaling and topological execution.
- Resume pending operations through chain and mempool inspection.
- Verify every action before moving the active instance pointer.
- Route direct deploy through a synthetic one-resource apply.

Acceptance criteria:

- Fault injection at every transition resumes without duplicate deployment.
- A partial multi-resource apply records completed actions and resumes from the
  first incomplete action.
- Previous instances remain addressable by history after replacement.
- Failed verification never makes the new instance active.

### Milestone 6: dependencies, proxies, and beacons

- Implement mutable, proxy, beacon, and observed edge semantics.
- Add standard upgradeable proxy, upgradeable beacon, and beacon proxy drivers.
- Add capability checks, upgrade history, impact reporting, and rollback.
- Add storage-layout metadata and unsafe-upgrade policy.

Acceptance criteria:

- Upgrading a proxy keeps its ID and records the old and new implementation.
- Upgrading a beacon leaves every proxy untouched and reports the complete
  managed impact set.
- A consumer of a stable proxy ID is not replaced by an implementation update.
- A consumer pinned to an implementation ID is replaced.
- Unauthorized and storage-unsafe upgrades are blocked before broadcast.

### Milestone 7: production hardening and automation

- Add confirmation policies, maximum fee budgets, protected environments, and
  explicit approval for unsafe or destructive state-only operations.
- Implement at least one production remote backend with leases,
  compare-and-swap, versioning, access control, and recovery.
- Add an external signer or PSBT-compatible transaction flow.
- Add content-addressed saved plans and immutable deployment receipts.
- Add verified state recovery from receipts, `labcoat.lock`, and chain data.
- Add plan/apply/state MCP tools with stable schemas.
- Add state inspection and recovery guidance to generated project docs.

Acceptance criteria:

- Production apply refuses stale plans, insufficient confirmations, missing
  capabilities, stale indexer state, and fee budgets over policy.
- Mainnet apply is unavailable without a protected environment, supported
  production backend, reviewed plan digest, and supported signer.
- Signed transactions are rejected when they differ from approved transaction
  intent.
- A new workstation can recover verified resource state from remote state or
  from receipts plus chain observation without redeploying contracts.
- Every successful mainnet action emits a receipt sufficient to audit its code,
  intent, transaction, and resulting Alkane ID or implementation pointer.
- CLI and MCP produce equivalent plan and state results.

## Testing strategy

### Unit and property tests

- State schema round trips and version rejection.
- Atomic write and fail-closed corruption behavior.
- Lineage and monotonic serial invariants.
- Dependency reference parsing, edge typing, and cycle detection.
- Plan diff matrices for every resource and edge type.
- ABI method lookup and typed value encoding, including full-width `u128` IDs.
- Operation transition state-machine validity.
- Stale compare-and-swap rejection.

### Fault-injection tests

Interrupt apply before and after each durable transition:

- state prepared;
- commit broadcast;
- commit confirmation;
- reveal broadcast;
- reveal confirmation;
- Metashrew indexing;
- verification;
- active-pointer update;
- compatibility export.

Every case must either resume safely or stop with a specific recovery command.

### Real regtest integration tests

- Create and no-op reapply.
- Replace while retaining deployment history.
- Constructor dependency cascading replacement.
- Direct upgradeable proxy update with stable proxy state.
- Beacon update with multiple stable proxies.
- Reserved create and collision.
- Factory instance with independent storage.
- Authorization capability missing and present.
- Saved-plan transaction policy versus signed-transaction mismatch.
- Remote lease contention, expiry, stale serial, and point-in-time restore.
- Receipt generation and recovery on a clean workstation.
- Partial apply recovery.
- Metashrew lag.
- Snapshot/restore, reorg where practical, and full reset detection.
- Import and legacy migration.

## Migration and rollout

1. Ship read-only state inspection and explicit migration first.
2. If a v1 `labcoat.lock` exists without canonical state, mutating state-engine
   commands fail with a `labcoat state migrate` hint. Do not auto-migrate during
   deploy or apply.
3. Migration writes a timestamped backup before creating version 2 state.
4. Continue resolving names from v1 state and supporting existing direct
   commands during the transition.
5. Introduce declarative plan/apply as opt-in until crash recovery and real
   regtest tests pass.
6. Route direct deploy through the apply engine only after synthetic plans are
   behaviorally compatible.
7. Deprecate `lock migrate` in favor of `state migrate`, but retain it as an
   alias for at least one release cycle.

## Risks and mitigations

- **Commit broadcast without recorded txid:** refactor the executor to emit or
  return phase events before continuing; do not advertise crash safety first.
- **Sequential ID races:** never predict IDs; resolve them after indexed
  deployment and then continue the graph.
- **Concurrent applies:** exclusive backend lease plus state serial
  compare-and-swap. A future remote backend must provide equivalent semantics.
- **Reorgs and resets:** persist block hashes and devnet identity, refresh before
  mutation, and mark affected instances orphaned rather than deleting them.
- **Metashrew lag:** classify separately and block mutation until observation is
  reliable.
- **Proxy storage corruption:** require storage compatibility metadata or
  explicit unsafe approval.
- **Unknown hardcoded dependencies:** require explicit `depends_on`; never claim
  complete dependency discovery from Wasm.
- **Partial multi-resource apply:** journal each irreversible action and resume;
  never imply graph-wide atomicity.
- **State loss:** preserve `labcoat.lock` as an active address book, support
  verified import and receipt-based reconstruction, and require a versioned
  remote backend before recommending team or mainnet production use.
- **Signer compromise or transaction substitution:** separate plan,
  construction, signing, and broadcast; bind signed transactions to the
  approved plan digest and wallet fingerprint.
- **Unbounded mainnet spend:** require explicit fee and total-spend policies and
  show every BTC and Alkanes flow in the reviewed plan.

## Definition of done

Durable state is complete when:

- Repeated unchanged apply is a no-op.
- Replacements preserve all previous Alkane IDs and transaction history.
- Direct, reserved, factory, proxy, and beacon lifecycles plan correctly.
- Dependency changes produce correct replacement or upgrade behavior.
- Every broadcast operation is journaled and safely resumable.
- State corruption, stale plans, chain mismatch, reorg, reset, and Metashrew lag
  fail closed.
- Active state is updated only after chain and Metashrew verification.
- Upgrade plans report authorization, storage compatibility, and blast radius.
- The existing direct CLI remains compatible throughout migration.

## Mainnet-ready release gate

Durable state may ship for regtest and signet before mainnet is ready. Labcoat
must not document the feature as mainnet-ready until:

- a production remote backend is implemented and recovery-tested;
- an external signer or PSBT flow is implemented and substitution-tested;
- mainnet requires protected environments and reviewed saved plans;
- fee, spend, confirmation, capability, and upgrade policies fail closed;
- deployment receipts and clean-workstation recovery are tested;
- commit/reveal interruption recovery passes fault injection;
- at least one signet dress rehearsal exercises the complete production path.
