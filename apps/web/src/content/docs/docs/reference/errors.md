---
title: Errors and recovery
description: Stable Labcoat error codes and the first recovery action to take.
---

JSON errors use stable codes and always include a next-step hint.

Do not parse the human message to branch automation. Branch on `error.code` and
surface `error.hint` to the operator or agent.

## General

| Code | Meaning | First response |
| --- | --- | --- |
| `CONFIG_INVALID` | Project or environment configuration is invalid | Run `labcoat doctor` |
| `TOOLKIT_ERROR` | A local operation failed, such as reading or writing a project file | Check permissions and disk space; the message names the path |
| `RPC_UNREACHABLE` | The configured RPC endpoint cannot be reached | Run `labcoat status` |
| `INDEXER_LAG` | Indexed height did not reach chain height | Run `labcoat logs` |

## Labcoat Network and wallets

| Code | Meaning | First response |
| --- | --- | --- |
| `LABCOAT_NETWORK_ERROR` | A Labcoat Network operation failed | Run `labcoat status` and inspect `labcoat logs` |
| `BINARY_CRASH` | A Labcoat Network process exited | Inspect service logs |
| `WALLET_MISSING` | The selected network has no keystore | Run `labcoat up` on Labcoat Network, or `labcoat wallet init` for another network |
| `WALLET_LOCKED` | Passphrase is missing or incorrect | Set `LABCOAT_WALLET_PASSPHRASE` |
| `INSUFFICIENT_FUNDS` | Spendable BTC or single-asset token outputs cannot cover the operation | Fund and mine the wallet |

## Contracts and tests

| Code | Meaning | First response |
| --- | --- | --- |
| `TOOLCHAIN_MISSING` | Cargo, a Rust WebAssembly target, a WebAssembly-capable C compiler, or the WASI C headers are not installed | Install what the message names, then run `labcoat doctor` |
| `COMPILE_FAILED` | Rust or Wasm compilation failed | Read stderr and run `labcoat doctor` |
| `TEST_FAILED` | Host or e2e tests failed, or an e2e filter matched no test | Fix the failing tests named in the output |
| `CONTRACT_NOT_FOUND` | Name or ID could not be resolved | Run `labcoat lock show` |
| `LOCKFILE_INVALID` | `labcoat.lock` cannot be read or parsed | Restore it from version control |
| `ENVELOPE_INVALID` | The deploy payload is not a raw Wasm module | Pass the `.wasm` produced by `labcoat build` |
| `ABI_MISMATCH` | The local build's ABI differs from the deployed contract | Build the deployed source revision |
| `EXECUTION_REVERT` | The contract explicitly reverted | Inspect `revertReason` and trace |
| `TRACE_TIMEOUT` | A decoded trace did not arrive in time | Retry `labcoat trace --wait` |

## Deployment scripts

| Code | Meaning | First response |
| --- | --- | --- |
| `DEPLOY_SCRIPT_INVALID` | `deploy.rs` is missing, failed to build or run, or declared an invalid step | Read the message; `cargo run --example deploy` reproduces a build failure |
| `DEPLOYMENT_CONFLICT` | A step that already ran no longer matches `deploy.rs`, or references a contract that was replaced | Restore the step, or name it in `labcoat deploy --replace <name>` |
| `DEPLOYMENT_BLOCKED` | A step cannot proceed: an interrupted broadcast, an occupied reserve, an unreadable setting, or a step deployed on its own before the steps it references | Run `labcoat plan` and resolve the step's detail |
| `DEPLOYMENT_LOCKED` | Another deploy or import is updating the project's deployments | Retry after the other command finishes |
| `RUN_FILE_INVALID` | `deployments/<network>.json` cannot be read, parsed, or written | Restore it from version control |

## Atomic exchanges

| Code | Meaning | First response |
| --- | --- | --- |
| `EXCHANGE_REQUEST_INVALID` | The exchange request file is malformed | Regenerate it with the application that produced it |
| `EXCHANGE_PLAN_INVALID` | The request or plan breaks an exchange rule | Read the message; replan with valid assets and P2TR addresses |
| `EXCHANGE_PLAN_MISMATCH` | The PSBT differs from the plan | Use the PSBT produced with that plan |
| `EXCHANGE_ASSET_UNSAFE` | Asset routing would not deliver exactly the planned amounts | Replan; do not sign |
| `EXCHANGE_INPUT_OWNERSHIP` | An input's owner differs from the plan | Replan from the participants' wallets |
| `EXCHANGE_SELLER_DEBIT` | The seller would lose bitcoin | Replan; do not sign |
| `EXCHANGE_SIGHASH_UNSUPPORTED` | A signature uses a sighash other than `SIGHASH_DEFAULT` | Re-sign with `labcoat wallet sign-psbt` |
| `EXCHANGE_SIGNATURE_MISSING` | A required buyer signature is missing | Sign the buyer inputs, then settle |
| `EXCHANGE_SIGNATURE_INVALID` | A signature failed verification | Re-sign the original PSBT |
| `EXCHANGE_INPUT_SPENT` | A planned input has already been spent | Build a new plan |
| `EXCHANGE_TIP_STALE` | The chain tip seen at planning is no longer active | Build a new plan |
| `EXCHANGE_NETWORK_MISMATCH` | The live chain differs from the plan's chain | Plan and settle on the same network |

## Durable state

| Code | Meaning | First response |
| --- | --- | --- |
| `STATE_MISSING` | No durable state exists for this environment | Run `labcoat state migrate` |
| `STATE_INVALID` | The state file is corrupt or unreadable; it is never treated as empty | Restore `backups/state.json.prev` |
| `STATE_LOCKED` | Another labcoat process holds the environment lease | Wait; a crashed holder releases it automatically |
| `STATE_CHAIN_MISMATCH` | Durable state belongs to a different chain instance | Archive `.labcoat/state/<environment>` or switch `--environment` |
| `STATE_CONFLICT` | Durable state changed underneath the command, or already exists | Re-run against current state (`labcoat state list`) |
| `STATE_UNSUPPORTED` | The durable state schema version is not supported | Upgrade labcoat or restore a backup |
