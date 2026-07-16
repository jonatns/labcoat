import { access, mkdir, readFile, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const webRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = path.resolve(webRoot, '../..');
const check = process.argv.includes('--check');
const binIndex = process.argv.indexOf('--bin');
const binary = binIndex >= 0 ? path.resolve(repoRoot, process.argv[binIndex + 1]) : null;

function runDocs(json = false) {
  const command = binary ?? 'cargo';
  const args = binary
    ? ['docs', ...(json ? ['--json'] : ['--llm'])]
    : ['run', '--quiet', '-p', 'labcoat-cli', '--', 'docs', ...(json ? ['--json'] : ['--llm'])];
  const result = spawnSync(command, args, {
    cwd: repoRoot,
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'inherit'],
  });
  if (result.status !== 0) throw new Error(`Reference generation failed with status ${result.status}`);
  return result.stdout.trim();
}

const markdown = runDocs(false).replace(/^# Labcoat[^\n]*\n+/, '');
const envelope = JSON.parse(runDocs(true));
const generatedMarkdown = `---
title: CLI reference
description: Generated command, option, MCP tool, and protocol reference for Labcoat.
editUrl: false
---

> Generated from Labcoat ${envelope.result.version}. Run \`pnpm sync:reference\` after changing CLI or MCP metadata.

${markdown}
`;
const generatedJson = `${JSON.stringify(envelope, null, 2)}\n`;
const targets = [
  [path.join(webRoot, 'src/content/docs/docs/reference/cli.md'), generatedMarkdown],
  [path.join(webRoot, 'src/generated/cli-reference.json'), generatedJson],
];

for (const [target, content] of targets) {
  if (check) {
    try {
      await access(target);
      const existing = await readFile(target, 'utf8');
      if (existing !== content) throw new Error(`${path.relative(repoRoot, target)} is stale; run pnpm sync:web-reference`);
    } catch (error) {
      if (error instanceof Error && error.message.includes('is stale')) throw error;
      throw new Error(`${path.relative(repoRoot, target)} is missing; run pnpm sync:web-reference`);
    }
  } else {
    await mkdir(path.dirname(target), { recursive: true });
    await writeFile(target, content);
    console.log(`Updated ${path.relative(repoRoot, target)}`);
  }
}
