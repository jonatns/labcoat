import { readFile } from 'node:fs/promises';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect, test } from '@playwright/test';
import sharp from 'sharp';

const webRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = path.resolve(webRoot, '../..');
const readRepo = (relativePath: string) => readFile(path.join(repoRoot, relativePath), 'utf8');

test('brand, agent, release, and generated asset contracts are synchronized', async ({}, testInfo) => {
  test.skip(testInfo.project.name === 'mobile', 'structural contract needs one execution');

  for (const command of [
    ['scripts/validate-brand.mjs'],
    ['apps/web/scripts/generate-og.mjs', '--check'],
    ['scripts/tests/release-notes-test.mjs'],
  ]) {
    const result = spawnSync(process.execPath, command, { cwd: repoRoot, encoding: 'utf8' });
    expect(result.status, result.stderr || result.stdout).toBe(0);
  }

  const brand = JSON.parse(await readRepo('brand.json'));
  const cliReference = JSON.parse(await readRepo('apps/web/src/generated/cli-reference.json'));
  const skill = await readRepo('skills/SKILL.md');
  const packageJson = JSON.parse(await readRepo('apps/web/package.json'));
  const css = await readRepo('apps/web/src/styles/global.css');
  const astroConfig = await readRepo('apps/web/astro.config.ts');

  expect(packageJson.dependencies['@fontsource-variable/ibm-plex-sans']).toBe('5.2.8');
  expect(packageJson.dependencies['@fontsource/ibm-plex-mono']).toBe('5.2.7');
  expect(css).toContain("@import './fonts.css'");
  expect(await readRepo('apps/web/src/styles/fonts.css')).toContain('ibm-plex-sans-latin-wght-normal.woff2');
  expect(css).not.toMatch(/https?:\/\/.+(?:woff|font)/i);
  expect(astroConfig).toContain("dark: './src/assets/labcoat-mark.svg'");
  expect(astroConfig).toContain("light: './src/assets/labcoat-mark-light.svg'");
  expect(await readRepo('apps/web/src/assets/labcoat-mark.svg')).not.toContain('currentColor');
  expect(await readRepo('apps/web/src/assets/labcoat-mark-light.svg')).not.toContain('currentColor');

  const topLevel = new Set(cliReference.result.commands.map((command: { name: string }) => command.name));
  for (const fence of skill.matchAll(/```(?:bash|sh|shell)?\s*\n([\s\S]*?)```/g)) {
    for (const match of fence[1].matchAll(/\blabcoat\s+([^\s\\]+)/g)) {
      const command = match[1].replace(/[;&|]+$/, '');
      if (!command.startsWith('-')) expect(topLevel.has(command), `skill command: ${command}`).toBeTruthy();
    }
  }
  expect(skill).not.toMatch(/\blabcoat\s+(?:compile|contract)\b/);

  const generatedBlock = skill.match(/<!-- BEGIN GENERATED MCP TOOLS -->([\s\S]*?)<!-- END GENERATED MCP TOOLS -->/)?.[1] ?? '';
  const skillTools = [...generatedBlock.matchAll(/^- `([^`]+)`/gm)].map((match) => match[1]);
  expect(skillTools).toEqual(cliReference.result.mcpTools.map((tool: { name: string }) => tool.name));

  for (const file of ['README.md', 'apps/web/src/content/docs/docs/getting-started/installation.md', 'apps/web/src/content/docs/docs/reference/stability.md']) {
    const source = await readRepo(file);
    expect(source).toContain('cli-v0.1.0');
    expect(source).toContain('labcoat compile');
    expect(source).toContain('labcoat build');
  }

  const metadata = await sharp(path.join(webRoot, 'public/og.png')).metadata();
  expect(metadata.width).toBe(1200);
  expect(metadata.height).toBe(630);
  expect(await readRepo('apps/web/public/og.svg')).toContain(brand.socialHeadline);

  for (const file of ['SECURITY.md', 'CONTRIBUTING.md', 'apps/web/src/content/docs/docs/reference/stability.md']) {
    expect((await readRepo(file)).length).toBeGreaterThan(300);
  }
});
