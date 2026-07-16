---
title: Automation and agents
description: Control Labcoat through JSON envelopes, MCP tools, and machine-readable documentation.
---

Labcoat treats automation as a public interface rather than a shell-screen
scraping exercise.

## JSON mode

Every command accepts `--json` and emits exactly one envelope on stdout. Logs
and progress stay on stderr.

```bash
labcoat status --json
labcoat deploy build/MyToken.wasm --dry-run --json
```

```json
{
  "ok": false,
  "command": "deploy",
  "schema": "labcoat/v1/error",
  "error": {
    "code": "WALLET_MISSING",
    "message": "project wallet does not exist",
    "hint": "run `labcoat wallet init` first"
  }
}
```

An envelope being printed produces exit code 0, even when `ok` is false. Read
the envelope rather than inferring application success from the process code.

## MCP mode

```bash
labcoat mcp serve
```

The stdio MCP server exposes devnet, wallet, compilation, deployment, call,
simulation, and trace tools using the same typed operations as the CLI.

## Context endpoints

- [`/llms.txt`](/llms.txt) is the concise index.
- [`/llms-full.txt`](/llms-full.txt) contains the full public documentation.
- Every docs page has a sibling `.md.txt` URL.
- [`/reference/cli.json`](/reference/cli.json) is the structured reference.
- [`/skill.md`](/skill.md) is the canonical workflow for coding agents.
