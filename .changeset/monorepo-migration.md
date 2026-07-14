---
"@jonatns/labcoat": minor
---

Rebuilt on a pinned alkanes-rs (develop) Rust core — oyl-sdk removed
entirely. Public API unchanged (`labcoat.setup()` → deploy/simulate/
execute); wallet/deploy/execute/simulate/trace now run through the
`labcoat` CLI. Deployments recorded in labcoat.lock (run
`labcoat lock migrate` once); deploy consumes the raw `.wasm` artifact;
`network: "oylnet"` is deprecated in favor of `regtest`.
