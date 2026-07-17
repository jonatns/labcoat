import { mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { spawnSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const directory = await mkdtemp(path.join(tmpdir(), 'labcoat-release-notes-'));
const changelog = path.join(directory, 'CHANGELOG.md');
const output = path.join(directory, 'notes.md');

await writeFile(changelog, `# Changelog

## [1.2.3] - 2026-07-17

### Added

- add a deterministic trace view

### Breaking changes

- rename an example command

## [1.2.2]

- older change
`);

const result = spawnSync(process.execPath, [
  'scripts/release/render-release-notes.mjs',
  '--version', '1.2.3',
  '--tag', 'cli-v1.2.3',
  '--changelog', changelog,
  '--output', output,
], { cwd: repoRoot, encoding: 'utf8' });

if (result.status !== 0) throw new Error(result.stderr || 'release note renderer failed');
const notes = await readFile(output, 'utf8');
for (const expected of [
  'add a deterministic trace view',
  'Compatibility and breaking changes',
  'rename an example command',
  'curl -fsSL https://labcoat.sh/install',
  'gh attestation verify',
  'Known limitations',
  'full changelog',
]) {
  if (!notes.includes(expected)) throw new Error(`rendered notes missing: ${expected}`);
}

console.log('Release note renderer test passed.');
