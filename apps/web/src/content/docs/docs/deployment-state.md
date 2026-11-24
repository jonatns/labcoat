---
title: Deployment state
description: Keep a durable, per-environment history of every deployment and detect reset chains before broadcasting.
---

`labcoat.lock` records the active id for each contract on each network. A
redeploy replaces the previous entry. Durable state adds what the lockfile
cannot hold: an append-only history of every instance, a chain identity, and
safe concurrent access.

Durable state is opt-in and currently intended for local development. It is
not yet shared, remote, or mainnet-ready state.

## Enable it

```bash
labcoat state migrate
```

Migration copies the active network's `labcoat.lock` records into
`.labcoat/state/<environment>/state.json` as imported instances. It backs up
`labcoat.lock` first and refuses to overwrite existing state. Projects that
never migrate behave exactly as before.

From then on, every `labcoat deploy` appends the new instance to
the contract's history. `labcoat.lock` stays in place and
remains the source for name resolution.

## Inspect it

```bash
labcoat state list
labcoat state show counter
labcoat state show counter --history
```

`state list` shows every resource and its active instance. `state show`
accepts a bare contract name or a resource address such as
`contract.counter`; `--history` includes every earlier instance, so a
redeploy shows `instance-2` without losing `instance-1`.

## Environments

Each environment has its own state and binds to one chain. The default is
`default`; choose another with `--environment`, `LABCOAT_ENVIRONMENT`, or
`environment = "..."` in `labcoat.toml`.

```bash
labcoat --environment staging state migrate
```

To target a second network or chain, use a second environment instead of
re-pointing an existing one.

## Reset protection

State records the network, the hash of block 1, and the Labcoat Network
instance id shown by `labcoat status`. `labcoat reset` creates a new instance
id. After a reset, the next deploy fails with `STATE_CHAIN_MISMATCH` before
broadcasting, instead of mixing two chains' histories. Archive the environment
directory, or switch to a fresh `--environment`, to continue on the new chain.

## Safety guarantees

- **Atomic writes:** a crash mid-write leaves either the previous or the new
  state file, never a partial one. The previous version is kept in
  `backups/state.json.prev`.
- **Fail closed:** a corrupt file is `STATE_INVALID` and an unknown schema is
  `STATE_UNSUPPORTED`; neither is ever read as empty state.
- **One writer:** mutating commands hold an exclusive lease on `state.lock`,
  and a second process gets `STATE_LOCKED`. The operating system releases the
  lease if a process crashes, so never delete `state.lock` while Labcoat runs.

## Files

```text
labcoat.lock                              active ids per network
deployments/<network>.json                deploy.rs run file
.labcoat/state/<environment>/state.json   durable state
.labcoat/state/<environment>/state.lock   environment lease
.labcoat/state/<environment>/backups/     previous state and migration backups
```
