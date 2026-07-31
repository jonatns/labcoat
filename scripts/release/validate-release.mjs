#!/usr/bin/env node

import { createHash } from 'node:crypto';
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { resolve } from 'node:path';

const root = resolve(import.meta.dirname, '../..');
const read = (path) => readFileSync(resolve(root, path), 'utf8');
const sourcesPath = 'crates/labcoat-cli/runtime-sources.json';
const runtime = JSON.parse(read(sourcesPath));
const semver = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/;
const gitCommit = /^[0-9a-f]{40}$/;
const cliTag = /^cli-v\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/;

function fail(message) {
  throw new Error(message);
}

function cargoPackage(path) {
  const source = read(path);
  const packageBlock = source.match(/\[package\]([\s\S]*?)(?=\n\[|$)/)?.[1] ?? '';
  return { source, packageBlock };
}

function workspaceVersion() {
  const source = read('Cargo.toml');
  const block = source.match(/\[workspace\.package\]([\s\S]*?)(?=\n\[|$)/)?.[1];
  const version = block?.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
  if (!version || !semver.test(version)) fail('Cargo.toml has no valid workspace package version');
  return version;
}

function sourceDigest() {
  return createHash('sha256')
    .update(JSON.stringify({ sources: runtime.sources, compatibility: runtime.compatibility }))
    .digest('hex');
}

function validateCargo() {
  const version = workspaceVersion();
  for (const name of ['labcoat-cli', 'labcoat-core', 'labcoat-test', 'isomer-core']) {
    const pkg = cargoPackage(`crates/${name}/Cargo.toml`).packageBlock;
    if (!/^version\.workspace\s*=\s*true$/m.test(pkg)) {
      fail(`${name} must inherit the workspace version`);
    }
    if (!/^publish\s*=\s*false$/m.test(pkg)) {
      fail(`${name} must not be published to a registry`);
    }
  }

  const template = read('crates/labcoat-cli/templates/default/Cargo.toml');
  if (!template.includes('labcoat-test = { git = "https://github.com/jonatns/labcoat", tag = "cli-v{{LABCOAT_VERSION}}" }')) {
    fail('project template must pin labcoat-test to the matching Labcoat release tag');
  }
  if (template.includes('labcoat-test = "=')) {
    fail('project template must not resolve labcoat-test from crates.io');
  }
  if (!template.includes('metashrew-support = { git = "https://github.com/kungfuflex/metashrew", tag = "v9.0.5-rc.8" }')) {
    fail('project template must use the same metashrew source as alkanes-rs');
  }
  if (template.includes('sandshrewmetaprotocols/metashrew')) {
    fail('project template uses the legacy metashrew remote');
  }

  const releaseConfig = read('release-plz.toml');
  if (!releaseConfig.includes('name = "labcoat-cli"')) {
    fail('release-plz must use labcoat-cli as the product release owner');
  }
  if (releaseConfig.includes('name = "labcoat-test"')) {
    fail('release-plz must not use labcoat-test as the release owner');
  }
  return version;
}

function validateRuntimeSources() {
  if (runtime.schema !== 1) fail(`${sourcesPath} schema must be 1`);
  const sources = Object.entries(runtime.sources ?? {});
  if (sources.length !== 3) fail(`${sourcesPath} must declare exactly three build sources`);
  for (const [name, source] of sources) {
    if (!source.repository || !source.revision) fail(`incomplete runtime source: ${name}`);
    if (!gitCommit.test(source.revision)) fail(`${name} revision must be an immutable commit`);
  }
  const expectedSources = {
    qubitcoin: 'e7f2f9d8844bdc7662030d98abb0544cc3e5a8da',
    'alkanes-wasm': '714843c416e2ab57352a33f05b8461cf3f540f5a',
    'esplorashrew-wasm': '7f7660908cdb54d12540ac6a8b337ef6a70e8057',
  };
  for (const [name, revision] of Object.entries(expectedSources)) {
    if (runtime.sources[name]?.revision !== revision) {
      fail(`${name} must use the approved revision`);
    }
  }
  if (!gitCommit.test(runtime.compatibility?.qubitcoin_metashrew_revision ?? '')) {
    fail('runtime sources must pin the Qubitcoin-compatible Metashrew commit');
  }
}

function validateActions() {
  for (const name of readdirSync(resolve(root, '.github/workflows'))) {
    if (!name.endsWith('.yml') && !name.endsWith('.yaml')) continue;
    const source = read(`.github/workflows/${name}`);
    for (const match of source.matchAll(/^\s*(?:-\s+)?uses:\s*([^\s#]+)/gm)) {
      const target = match[1];
      if (target.startsWith('./')) continue;
      if (!/@[0-9a-f]{40}$/.test(target)) {
        fail(`${name} uses an action that is not pinned to a full commit SHA: ${target}`);
      }
    }
  }
}

function validateReleaseWorkflows() {
  const prepare = read('.github/workflows/release-pr.yml');
  if (!/on:\s*\n\s+workflow_dispatch:/m.test(prepare) || /^\s+push:/m.test(prepare)) {
    fail('release-pr.yml must be manual-only');
  }
  if (prepare.includes('update-release-trigger')) {
    fail('release-pr.yml must not update an artificial release trigger');
  }

  const release = read('.github/workflows/release-cli.yml');
  for (const required of [
    'runtime-manifest.json',
    'build-qubitcoind',
    'build-alkanes-wasm',
    'build-esplorashrew-wasm',
    'runtime-acceptance',
    'actions/attest-build-provenance',
    'scripts/release/build-runtime-manifest.mjs',
  ]) {
    if (!release.includes(required)) fail(`release-cli.yml is missing ${required}`);
  }
  for (const removed of ['cargo publish', 'crates-io-auth-action', 'runtime-promotion/']) {
    if (release.includes(removed)) fail(`release-cli.yml still contains obsolete flow: ${removed}`);
  }
  for (const obsolete of [
    '.github/workflows/release-runtime.yml',
    '.github/workflows/runtime-acceptance.yml',
    'runtime.json',
    'crates/labcoat-test/RELEASE_TRIGGER',
  ]) {
    if (existsSync(resolve(root, obsolete))) fail(`obsolete release input still exists: ${obsolete}`);
  }
}

function main() {
  const [command = 'validate', value] = process.argv.slice(2);
  if (command === '--workspace-version') return console.log(workspaceVersion());
  if (command === '--runtime-source-digest') return console.log(sourceDigest());
  if (command === '--validate-cli-tag') {
    if (!cliTag.test(value ?? '')) fail(`invalid CLI tag: ${value ?? ''}`);
    if (value !== `cli-v${workspaceVersion()}`) {
      fail(`${value} does not match workspace version ${workspaceVersion()}`);
    }
    return;
  }
  if (command !== 'validate') fail(`unknown command: ${command}`);
  const version = validateCargo();
  validateRuntimeSources();
  validateActions();
  validateReleaseWorkflows();
  console.log(`release metadata valid (Labcoat ${version}, runtime sources ${sourceDigest()})`);
}

main();
