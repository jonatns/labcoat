import { access, mkdir, readFile, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const webRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = path.resolve(webRoot, '../..');
const check = process.argv.includes('--check');
const binIndex = process.argv.indexOf('--bin');
const binary = binIndex >= 0 ? path.resolve(repoRoot, process.argv[binIndex + 1]) : null;
const skillPath = path.join(repoRoot, 'skills/SKILL.md');
const mcpStart = '<!-- BEGIN GENERATED MCP TOOLS -->';
const mcpEnd = '<!-- END GENERATED MCP TOOLS -->';

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

function generateMcpBlock(tools) {
  const rows = tools.map((tool) => `- \`${tool.name}\` — ${tool.description}`);
  return `${mcpStart}\n${rows.join('\n')}\n${mcpEnd}`;
}

function synchronizeSkill(source) {
  const markerPattern = new RegExp(`${mcpStart}[\\s\\S]*?${mcpEnd}`);
  if (!markerPattern.test(source)) {
    throw new Error('skills/SKILL.md is missing generated MCP tool markers');
  }
  return source.replace(markerPattern, generateMcpBlock(envelope.result.mcpTools));
}

function validateSkillCommands(source) {
  const topLevel = new Set(envelope.result.commands.map((command) => command.name));
  const removed = [...source.matchAll(/\blabcoat\s+(compile|contract)\b/g)].map((match) => match[1]);
  if (removed.length > 0) {
    throw new Error(`skills/SKILL.md uses removed top-level commands: ${[...new Set(removed)].join(', ')}`);
  }

  const invalid = [];
  const fences = source.matchAll(/```(?:bash|sh|shell)?\s*\n([\s\S]*?)```/g);
  for (const fence of fences) {
    for (const line of fence[1].split('\n')) {
      for (const command of line.matchAll(/\blabcoat\s+([^\s\\]+)/g)) {
        const topCommand = command[1].replace(/[;&|]+$/, '');
        if (topCommand.startsWith('-')) continue;
        if (!topLevel.has(topCommand)) invalid.push(topCommand);
      }
    }
  }
  if (invalid.length > 0) {
    throw new Error(`skills/SKILL.md has commands missing from generated CLI metadata: ${[...new Set(invalid)].join(', ')}`);
  }
}

const existingSkill = await readFile(skillPath, 'utf8');
const generatedSkill = synchronizeSkill(existingSkill);
validateSkillCommands(generatedSkill);

if (check) {
  if (existingSkill !== generatedSkill) {
    throw new Error('skills/SKILL.md MCP tool list is stale; run pnpm sync:web-reference');
  }
} else if (existingSkill !== generatedSkill) {
  await writeFile(skillPath, generatedSkill);
  console.log('Updated skills/SKILL.md');
}

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
