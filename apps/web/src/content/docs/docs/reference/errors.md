---
title: Errors and recovery
description: Stable Labcoat error codes and the first recovery action to take.
---

JSON errors use stable codes and always include a next-step hint.

| Code | Meaning | First response |
| --- | --- | --- |
| `CONFIG_INVALID` | Project or environment configuration is invalid | Run `labcoat doctor` |
| `WALLET_MISSING` | No project wallet exists | Run `labcoat wallet init` |
| `WALLET_LOCKED` | Passphrase is missing or incorrect | Set `LABCOAT_WALLET_PASSPHRASE` |
| `RPC_UNREACHABLE` | The configured gateway cannot be reached | Run `labcoat status` |
| `INDEXER_LAG` | Indexed height did not reach chain height | Inspect metashrew logs |
| `INSUFFICIENT_FUNDS` | Spendable BTC cannot cover the transaction | Fund and mine the wallet |
| `EXECUTION_REVERT` | The contract explicitly reverted | Inspect `revertReason` and trace |
| `TRACE_TIMEOUT` | A decoded trace did not arrive in time | Retry `labcoat trace --wait` |
| `COMPILE_FAILED` | Rust or Wasm compilation failed | Read stderr and run `labcoat doctor` |
| `CONTRACT_NOT_FOUND` | Name or ID could not be resolved | Run `labcoat lock show` |
| `BINARY_CRASH` | A managed devnet process exited | Inspect service logs |

Do not parse the human message to branch automation. Branch on `error.code` and
surface `error.hint` to the operator or agent.
