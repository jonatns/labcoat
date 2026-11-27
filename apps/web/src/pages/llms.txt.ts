import type { APIRoute } from 'astro';
import brand from '../../../../brand.json';

const body = `# ${brand.name}

> ${brand.description}

${brand.maturityNotice}

${brand.docsChannelNotice}

Canonical site: https://labcoat.sh
Source: https://github.com/jonatns/labcoat

## Start here
- [Overview](https://labcoat.sh/docs/): Product model and supported interface
- [Start with an agent](https://labcoat.sh/docs/getting-started/agent-onboarding.md.txt): Plain-text preflight for a coding agent setting up Labcoat, and the prompt to give it (https://labcoat.sh/agent-prompt.txt)
- [Installation](https://labcoat.sh/docs/getting-started/installation/): Native macOS and Linux installation
- [Quick start](https://labcoat.sh/docs/getting-started/quickstart/): Empty directory to decoded trace
- [Tutorial: your first contract](https://labcoat.sh/docs/tutorials/first-contract/): Read the starter Counter, deploy and call it, then add a method
- [Tutorial: tokens and exchanges](https://labcoat.sh/docs/tutorials/token-exchange/): Write a token, deploy two copies with deploy.rs, and swap atomically
- [Tutorial: an upgradeable contract](https://labcoat.sh/docs/tutorials/upgradeable-contract/): Put a contract behind a beacon proxy and upgrade it
- [Tutorial: a web app](https://labcoat.sh/docs/tutorials/web-app/): Show a contract's state in a browser page and change it with a signed transaction, with no wallet extension
- [Automation](https://labcoat.sh/docs/automation/): JSON envelopes and MCP

## Build with Labcoat
- [Projects and configuration](https://labcoat.sh/docs/projects-configuration/): Layout, settings precedence, and labcoat.lock
- [Labcoat Network and wallets](https://labcoat.sh/docs/labcoat-network/): Local chain lifecycle and wallets
- [Contracts](https://labcoat.sh/docs/contracts/): Test, build, deploy, simulate, call, and trace
- [Deployment scripts](https://labcoat.sh/docs/deployments/): deploy.rs, plan/deploy/import, upgradeable contracts, and e2e tests
- [Atomic exchanges](https://labcoat.sh/docs/exchanges/): Two-wallet asset swaps and the PSBT ceremony
- [Web apps](https://labcoat.sh/docs/web-apps/): Generated TypeScript client: typed reads and wallet-signed writes
- [Deployment state](https://labcoat.sh/docs/deployment-state/): Durable per-environment deployment history

## Reference
- [CLI reference](https://labcoat.sh/docs/reference/cli/)
- [Protocol reference](https://labcoat.sh/docs/reference/protocol/)
- [Errors and recovery](https://labcoat.sh/docs/reference/errors/)
- [Stability and releases](https://labcoat.sh/docs/reference/stability/)
- [Machine-readable CLI reference](https://labcoat.sh/reference/cli.json)
- [Agent skill](https://labcoat.sh/skill.md)
- [Full documentation corpus](https://labcoat.sh/llms-full.txt)

## Conventions
- Prefer \`labcoat mcp serve\` when the host supports MCP.
- Otherwise pass \`--json\` and read the single \`labcoat/v1/*\` envelope on stdout.
- Never put mnemonics or passphrases on argv or in \`labcoat.toml\`.
- Run \`labcoat docs --llm\` for the reference embedded in the installed binary.
`;

export const GET: APIRoute = () =>
  new Response(body, {
    headers: { 'Content-Type': 'text/plain; charset=utf-8' },
  });
