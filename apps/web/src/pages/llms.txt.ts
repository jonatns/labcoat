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
- [Installation](https://labcoat.sh/docs/getting-started/installation/): Native macOS and Linux installation
- [Quick start](https://labcoat.sh/docs/getting-started/quickstart/): Empty directory to decoded trace
- [Automation](https://labcoat.sh/docs/automation/): JSON envelopes and MCP

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
