---
title: Protocol reference
description: Cellpacks, commit/reveal deployments, protostone outputs, indexing, and deployment IDs.
---

## Cellpacks

An Alkanes call is encoded as `[block, tx, opcode, ...args]`, where each value
is a `u128`. Short string arguments are packed little-endian into one `u128`.

## Deployment envelopes

Deployment targets cellpack `[1, 0]`, meaning “create a new alkane.” The raw
Wasm module is compressed and placed in a taproot witness envelope across a
commit/reveal transaction pair.

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
inspect `labcoat status` and `labcoat logs --service metashrew`.
