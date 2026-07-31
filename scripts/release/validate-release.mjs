#!/usr/bin/env node

import { readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';

const root = resolve(import.meta.dirname, '../..');
const read = (path) => readFileSync(resolve(root, path), 'utf8');
const sourcesPath = 'crates/labcoat-cli/runtime-sources.json';
const runtime = JSON.parse(read(sourcesPath));
const semver = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/;
const gitCommit = /^[0-9a-f]{40}$/;

function fail(message) {
  throw new Error(message);
}

function cargoPackage(path) {
  const source = read(path);
  return source.match(/\[package\]([\s\S]*?)(?=\n\[|$)/)?.[1] ?? '';
}

function findCargoManifests(directory) {
  const manifests = [];
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) {
      manifests.push(...findCargoManifests(path));
    } else if (entry.isFile() && entry.name === 'Cargo.toml') {
      manifests.push(path);
    }
  }
  return manifests;
}

function workspaceVersion() {
  const source = read('Cargo.toml');
  const block = source.match(/\[workspace\.package\]([\s\S]*?)(?=\n\[|$)/)?.[1];
  const version = block?.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
  if (!version || !semver.test(version)) fail('Cargo.toml has no valid workspace package version');
  return version;
}

function validateCargo() {
  const version = workspaceVersion();
  for (const name of ['labcoat-cli', 'labcoat-core', 'labcoat-test', 'isomer-core']) {
    const pkg = cargoPackage(`crates/${name}/Cargo.toml`);
    if (!/^version\.workspace\s*=\s*true$/m.test(pkg)) {
      fail(`${name} must inherit the workspace version`);
    }
    if (!/^publish\s*=\s*false$/m.test(pkg)) {
      fail(`${name} must not be published to a registry`);
    }
  }

  const templateManifests = findCargoManifests(
    resolve(root, 'crates/labcoat-cli/templates'),
  );
  if (templateManifests.length > 0) {
    const paths = templateManifests.map((path) => path.slice(root.length + 1));
    fail(
      `scaffold templates must not be named Cargo.toml because Cargo parses Git sources recursively: ${paths.join(', ')}`,
    );
  }

  const template = read('crates/labcoat-cli/templates/default/Cargo.toml.template');
  if (!template.includes('labcoat-test = { git = "https://github.com/jonatns/labcoat", tag = "cli-v{{LABCOAT_VERSION}}" }')) {
    fail('project template must pin labcoat-test to the matching Labcoat release tag');
  }
  if (!template.includes('metashrew-support = { git = "https://github.com/kungfuflex/metashrew", tag = "v9.0.5-rc.8" }')) {
    fail('project template must use the same metashrew source as alkanes-rs');
  }
  return version;
}

function validateRuntimeSources() {
  if (runtime.schema !== 1) fail(`${sourcesPath} schema must be 1`);
  const sources = Object.entries(runtime.sources ?? {});
  const requiredSources = ['qubitcoin', 'alkanes-wasm', 'esplorashrew-wasm'];
  if (
    sources.length !== requiredSources.length
    || requiredSources.some((name) => !runtime.sources?.[name])
  ) {
    fail(`${sourcesPath} must declare ${requiredSources.join(', ')}`);
  }
  for (const [name, source] of sources) {
    if (!source.repository || !source.revision) fail(`incomplete runtime source: ${name}`);
    if (!gitCommit.test(source.revision)) fail(`${name} revision must be an immutable commit`);
  }
  if (!gitCommit.test(runtime.compatibility?.qubitcoin_metashrew_revision ?? '')) {
    fail('runtime sources must pin the Qubitcoin-compatible Metashrew commit');
  }
}

function validateReleaseWorkflows() {
  const prepare = read('.github/workflows/release-pr.yml');
  if (!/on:\s*\n\s+workflow_dispatch:/m.test(prepare) || /^\s+push:/m.test(prepare)) {
    fail('release-pr.yml must be manual-only');
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
}

function main() {
  const [command = 'validate'] = process.argv.slice(2);
  if (command === '--workspace-version') return console.log(workspaceVersion());
  if (command !== 'validate') fail(`unknown command: ${command}`);
  const version = validateCargo();
  validateRuntimeSources();
  validateReleaseWorkflows();
  console.log(`release metadata valid (Labcoat ${version})`);
}

main();
