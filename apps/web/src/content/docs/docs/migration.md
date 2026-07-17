---
title: Migration from the TypeScript package
description: Move a legacy Labcoat project to the supported Rust-native CLI.
---

The retired TypeScript package and its legacy manifest are no longer the
supported interface. Back up the project before migrating.

## Recommended sequence

1. Install the current native CLI and run `labcoat doctor`.
2. Commit or copy the existing project and deployment metadata.
3. Run `labcoat lock migrate` once to convert legacy deployment records.
4. Review `labcoat.toml` and remove any secret material.
5. Run `labcoat test` and `labcoat build` against contract packages.
6. Start a clean local devnet and redeploy; old local-chain IDs are not portable.

Use `labcoat lock show` to verify the per-network ledger after migration. The
full repository audit remains available in the project’s `docs/migration`
directory.
