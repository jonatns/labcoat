---
title: Protocol reference
description: Cellpacks, commit/reveal deployments, protostone outputs, indexing, and deployment IDs.
---

## Cellpacks

An Alkanes call is encoded as `[block, tx, opcode, ...args]`, where each value
is a `u128`. With a named ABI method, a `String` parameter is packed
little-endian across as many `u128` cells as it needs. Raw numeric-opcode calls
accept short strings of up to 16 bytes, packed into one cell.

## Deployment envelopes

Deployment targets cellpack `[1, 0]`, meaning “create a new alkane at the next
free ID,” or `[3, N]` with `--reserve N` (or `reserve = N` in `alkanes.hcl`),
which creates it at the predictable ID `4:N`. The raw Wasm module is compressed
and placed in a taproot witness envelope across a commit/reveal transaction
pair.

## Contract IDs

The create trace returns the contract’s `block:tx` ID. Labcoat records it under
the chosen contract name in `labcoat.lock`, so later calls can use either form.

## Protostone outputs

Trace events attach to virtual outputs. For protostone index `i`, the output is:

```text
transaction_output_count + 1 + i
```

`labcoat trace` performs this mapping automatically.

## Index synchronization

State-changing operations wait until the Alkanes index height reaches the chain
height before reading new state. `INDEXER_LAG` means that bounded wait expired;
inspect `labcoat status` and `labcoat logs --service qubitcoind`.
