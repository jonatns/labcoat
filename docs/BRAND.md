# Labcoat brand platform

`brand.json` is the machine-readable source of truth for product naming and primary messaging. This document governs the judgment calls that do not fit in that manifest.

## Platform

| Element | Approved direction |
| --- | --- |
| Preferred name | **Labcoat**. Use **Labcoat CLI** only when disambiguating the interface. Use `labcoat` for the executable. |
| Category | Rust-native Alkanes development CLI and local environment. |
| Promise | Move from Rust source to a decoded trace through one deterministic command system. |
| Differentiator | The same interface owns contracts, the managed local chain stack, wallets, inspection, JSON, and MCP. |
| Personality | Precise, capable, investigative, restrained. |
| Emotional benefit | Confidence and control over a fragile multi-service workflow. |
| Primary tagline | **From Rust source to decoded trace.** |
| Supporting proof | **The chain is part of the tool.** |
| Expressive CTA | **Put the protocol on your bench.** Use only as an invitation, never as the literal product description. |

Labcoat is for local Alkanes development. It is not described as a general Bitcoin platform, a hosted service, or a production deployment system.

## Controlled vocabulary

| Prefer | Meaning and use | Avoid or qualify |
| --- | --- | --- |
| Labcoat | Product and brand name. | “LabCoat,” “Lab Coat,” or all-caps body copy. |
| Labcoat CLI | Interface name when context requires it. | Using it as the default product name. |
| `labcoat` | Executable, commands, and terminal examples. | Decorative renaming of commands. |
| Alkanes smart contracts | Primary technical object. | Generic “Bitcoin apps” claims. |
| Rust-native | Rust authoring, native Wasm tests, and Rust tooling. | Implying the complete runtime is a single Rust process. |
| managed local Bitcoin devnet | Labcoat-managed regtest services. | “One binary,” “full stack,” or a hosted/testnet implication. |
| local environment | The orchestrated development runtime. | Production environment. |
| command system | CLI, JSON, MCP, and versioned references exposing one capability set. | “Always synchronized” without a version boundary. |
| package-name deployment | Deployment behavior on the current main branch. | Presenting it as available in `cli-v0.1.0`. |
| decoded trace | Human- or machine-readable execution inspection. | Generic observability claims. |
| developers and agents | People and automated clients using the same capabilities. | “Agent native.” |
| early-stage / pre-1.0 | Current maturity. | Stable, production-ready, or mainnet-ready. |
| current main branch | Public web documentation channel. | Calling it stable release documentation. |
| installed-version reference | Output of `labcoat docs --llm`. | Assuming the website matches an installed release. |

Capability statements must be classified as current-main, release-dependent, planned, or unsupported. Durable state and production mainnet controls are planned, not shipped. Windows is unsupported.

## Voice and tone

| Surface | Voice | Required behavior |
| --- | --- | --- |
| Landing page | Clear, confident, restrained. | Lead with category and workflow. Pair expressive language with literal proof. |
| Documentation | Direct and procedural. | State prerequisites, channel/version, limitations, and recovery steps. |
| CLI help | Compact and literal. | Describe what the command does; preserve stable terminology. |
| Errors | Calm and diagnostic. | Name the failed operation, likely cause, and next action. No laboratory metaphors. |
| JSON and MCP | Deterministic and explicit. | Keep language stable, parseable, and consistent with generated metadata. |
| Release notes | Candid and operational. | Call out compatibility, verification, and known limitations. |
| Security guidance | Specific and sober. | Define local wallet/runtime boundaries and private reporting. |
| Social / repository | Distinctive but accurate. | Pair “Labcoat” with “Alkanes” or `labcoat.sh` for searchability. |

Use sentence case. Prefer short declarative sentences and concrete verbs. Do not inflate capability to create momentum.

## Metaphor boundaries

- Retain: bench, trace, instruments, inspection, controlled environment, reproducibility.
- Use sparingly: experiment, laboratory, protocol.
- Keep literal instructions literal. Do not rename commands theatrically, use “ship” for unqualified production deployment, or put metaphors in errors and recovery steps.

## Visual system

- Preserve the flask geometry and acid-green palette.
- Use IBM Plex Sans Variable for interface and editorial text, and IBM Plex Mono for commands, code, identifiers, and numeric readouts.
- Use the trace waveform inside the flask as the recurring graphic motif. Measurement grids, instrument readouts, and explicit system connectors support the controlled-environment idea.
- Use OKLCH color tokens. Success, warning, error, and information states always include text or an icon so color is not the only cue.
- Preserve clear space around the mark equal to the flask neck width. Do not distort, rotate, add effects, or combine the mark with another symbol.
- Use the full-color mark on approved dark or light backgrounds. Use the monochrome variant when reproduction is single-color. Use the small-size variant at 32px and below.

## Governance

Generated CLI metadata is authoritative for commands, schemas, capabilities, MCP tools, and errors. `brand.json` is authoritative for the name, category, primary messages, CTAs, and maturity notice. CI validates surfaces that cannot import either source directly.
