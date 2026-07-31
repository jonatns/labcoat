#!/usr/bin/env node

import { readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

const args = process.argv.slice(2);
const valueFor = (name) => {
  const index = args.indexOf(name);
  return index >= 0 ? args[index + 1] : undefined;
};

const root = resolve(valueFor('--root') ?? resolve(import.meta.dirname, '../..'));
const version = valueFor('--version');
const date = valueFor('--date') ?? new Date().toISOString().slice(0, 10);
const semver = /^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?$/;

function fail(message) {
  throw new Error(message);
}

function read(path) {
  return readFileSync(resolve(root, path), 'utf8');
}

function parseVersion(value, label) {
  const match = value?.match(semver);
  if (!match) fail(`${label} must be a valid SemVer version`);
  return {
    value,
    parts: match.slice(1, 4).map(Number),
    prerelease: match[4],
  };
}

function isAfter(next, current) {
  for (let index = 0; index < 3; index += 1) {
    if (next.parts[index] !== current.parts[index]) {
      return next.parts[index] > current.parts[index];
    }
  }
  if (next.prerelease === current.prerelease) return false;
  if (!next.prerelease) return true;
  if (!current.prerelease) return false;
  return next.prerelease.localeCompare(current.prerelease, undefined, { numeric: true }) > 0;
}

const next = parseVersion(version, '--version');
if (!/^\d{4}-\d{2}-\d{2}$/.test(date)) fail('--date must use YYYY-MM-DD');

const cargoPath = resolve(root, 'Cargo.toml');
const cargo = read('Cargo.toml');
const versionPattern = /(\[workspace\.package\][\s\S]*?^version\s*=\s*")([^"]+)(")/m;
const currentValue = cargo.match(versionPattern)?.[2];
const current = parseVersion(currentValue, 'workspace version');
if (!isAfter(next, current)) fail(`${next.value} must be newer than ${current.value}`);

const changelogPath = resolve(root, 'CHANGELOG.md');
const changelog = read('CHANGELOG.md');
if (changelog.includes(`## [${next.value}]`)) fail(`CHANGELOG.md already contains ${next.value}`);
if (!changelog.includes('## [Unreleased]\n')) fail('CHANGELOG.md has no Unreleased section');

const releaseHeader = `## [${next.value}](https://github.com/jonatns/labcoat/compare/cli-v${current.value}...cli-v${next.value}) - ${date}`;
const updatedCargo = cargo.replace(versionPattern, `$1${next.value}$3`);
const updatedChangelog = changelog.replace(
  '## [Unreleased]\n',
  `## [Unreleased]\n\n${releaseHeader}\n`,
);

writeFileSync(cargoPath, updatedCargo);
writeFileSync(changelogPath, updatedChangelog);
console.log(`prepared Labcoat ${next.value}`);
