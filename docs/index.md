# Labcoat documentation

Labcoat is a native Rust CLI for developing Alkanes smart contracts on
Bitcoin. It owns project scaffolding, contract testing, compilation,
wallets, Labcoat Network, deployment, calls, simulation, and tracing.

## Start here

- [README and quick start](../README.md)
- [Releasing Labcoat](RELEASING.md)
- [Durable state (Milestone 1, shipped for local development)](STATE.md)
- [Durable state design (later milestones proposed)](DURABLE-STATE-PLAN.md)
- [Toolchain and upstream pins](../TOOLCHAIN.md)
- [ADR 0001: an Alkanes development runtime](adr/0001-alkanes-development-runtime.md)
- [Agent workflow](../skills/SKILL.md)

For the complete command and protocol reference, run:

```bash
labcoat docs --llm
```

## Architecture

```text
crates/isomer-core   Labcoat Network lifecycle: runtime asset, process, faucet
crates/labcoat-core  contract toolkit on pinned alkanes-rs
crates/labcoat-cli   CLI, JSON envelopes, and MCP server
crates/labcoat-deploy deployment script API
crates/labcoat-test  native WebAssembly contract test harness
crates/labcoat-runtime Labcoat Network: chain, validation, Alkanes indexer host, JSON-RPC
```

The `labcoat` binary is the supported public interface to these crates.
