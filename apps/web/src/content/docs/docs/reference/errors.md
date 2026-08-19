---
title: Errors and recovery
description: Stable Labcoat error codes and the first recovery action to take.
---

JSON errors use stable codes and always include a next-step hint.

| Code | Meaning | First response |
| --- | --- | --- |
| `LABCOAT_NETWORK_ERROR` | A Labcoat Network operation failed | Run `labcoat status` and inspect `labcoat logs` |
| `CONFIG_INVALID` | Project or environment configuration is invalid | Run `labcoat doctor` |
| `WALLET_MISSING` | No project wallet exists | Run `labcoat wallet init` |
| `WALLET_LOCKED` | Passphrase is missing or incorrect | Set `LABCOAT_WALLET_PASSPHRASE` |
| `RPC_UNREACHABLE` | The configured Qubitcoin endpoint cannot be reached | Run `labcoat status` |
| `INDEXER_LAG` | Indexed height did not reach chain height | Inspect `qubitcoind` logs |
| `INSUFFICIENT_FUNDS` | Spendable BTC cannot cover the transaction | Fund and mine the wallet |
| `EXECUTION_REVERT` | The contract explicitly reverted | Inspect `revertReason` and trace |
| `TRACE_TIMEOUT` | A decoded trace did not arrive in time | Retry `labcoat trace --wait` |
| `COMPILE_FAILED` | Rust or Wasm compilation failed | Read stderr and run `labcoat doctor` |
| `CONTRACT_NOT_FOUND` | Name or ID could not be resolved | Run `labcoat lock show` |
| `STATE_MISSING` | No durable state exists for this environment | Run `labcoat state migrate` |
| `STATE_LOCKED` | Another labcoat process holds the environment lease | Wait; a crashed holder releases it automatically |
| `STATE_CHAIN_MISMATCH` | Durable state belongs to a different chain instance | Archive `.labcoat/state/<environment>` or switch `--environment` |
| `STATE_CONFLICT` | Durable state changed underneath the command, or already exists | Re-run against current state (`labcoat state list`) |
| `STATE_UNSUPPORTED` | The durable state schema version is not supported | Upgrade labcoat or restore a backup |
| `BINARY_CRASH` | A Labcoat Network process exited | Inspect service logs |

Do not parse the human message to branch automation. Branch on `error.code` and
surface `error.hint` to the operator or agent.
