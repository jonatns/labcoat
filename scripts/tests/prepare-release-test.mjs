#!/usr/bin/env node

import { execFileSync, spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { resolve } from 'node:path';

const root = resolve(import.meta.dirname, '../..');
const script = resolve(root, 'scripts/release/prepare-release.mjs');
const fixture = mkdtempSync(resolve(tmpdir(), 'labcoat-prepare-release-'));

writeFileSync(
  resolve(fixture, 'Cargo.toml'),
  '[workspace]\n\n[workspace.package]\nversion = "0.1.0"\n',
);
writeFileSync(
  resolve(fixture, 'CHANGELOG.md'),
  '# Changelog\n\n## [Unreleased]\n\n### Added\n\n- release preparation\n\n## [0.1.0]\n',
);

execFileSync(
  process.execPath,
  [script, '--root', fixture, '--version', '0.2.0', '--date', '2026-07-31'],
  { stdio: 'pipe' },
);

const cargo = readFileSync(resolve(fixture, 'Cargo.toml'), 'utf8');
if (!cargo.includes('version = "0.2.0"')) throw new Error('workspace version was not updated');

const changelog = readFileSync(resolve(fixture, 'CHANGELOG.md'), 'utf8');
if (!changelog.includes(
  '## [0.2.0](https://github.com/jonatns/labcoat/compare/cli-v0.1.0...cli-v0.2.0) - 2026-07-31',
)) {
  throw new Error('release changelog heading was not added');
}

for (const invalid of ['0.2.0', '0.1.0', 'not-semver']) {
  const result = spawnSync(
    process.execPath,
    [script, '--root', fixture, '--version', invalid, '--date', '2026-07-31'],
    { stdio: 'pipe' },
  );
  if (result.status === 0) throw new Error(`invalid release version was accepted: ${invalid}`);
}

console.log('release preparation tests passed');
