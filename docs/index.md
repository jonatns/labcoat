# Labcoat documentation

Labcoat is a native Rust CLI for developing Alkanes smart contracts on
Bitcoin. It owns project scaffolding, contract testing, compilation,
wallets, the local devnet, deployment, calls, simulation, and tracing.

## Start here

- [README and quick start](../README.md)
- [Migrating from the retired TypeScript package](MIGRATING.md)
- [Releasing the CLI](RELEASING.md)
- [Toolchain and upstream pins](../TOOLCHAIN.md)
- [Agent workflow](../skills/SKILL.md)

For the complete command and protocol reference, run:

```bash
labcoat docs --llm
```

## Architecture

```text
crates/isomer-core   headless devnet process and chain control
crates/labcoat-core  contract toolkit on pinned alkanes-rs
crates/labcoat-cli   CLI, JSON envelopes, and MCP server
crates/labcoat-test  native WebAssembly contract test harness
```

The `labcoat` binary is the supported public interface to these crates.
