#!/usr/bin/env node

import { createHash } from 'node:crypto';
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { resolve } from 'node:path';
import process from 'node:process';
import { execFileSync } from 'node:child_process';

const root = resolve(import.meta.dirname, '../..');
const read = (path) => readFileSync(resolve(root, path), 'utf8');
const manifest = JSON.parse(read('runtime.json'));
const semver = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/;
const sha256 = /^[0-9a-f]{64}$/;
const gitCommit = /^[0-9a-f]{40}$/;
const cliTag = /^cli-v\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/;
const runtimeTag = /^runtime-v\d{4}\.\d{2}\.\d{2}\.\d+$/;

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
  return createHash('sha256').update(JSON.stringify(manifest.sources)).digest('hex');
}

function releaseTriggerDigest() {
  const trigger = 'crates/labcoat-test/RELEASE_TRIGGER';
  const files = execFileSync(
    'git',
    ['ls-files', '-z', 'crates/labcoat-cli', 'crates/labcoat-core', 'crates/labcoat-test'],
    { cwd: root },
  )
    .toString()
    .split('\0')
    .filter((path) => path && path !== trigger && existsSync(resolve(root, path)))
    .sort();
  const hash = createHash('sha256');
  for (const path of files) {
    hash.update(path);
    hash.update('\0');
    hash.update(readFileSync(resolve(root, path)));
    hash.update('\0');
  }
  return hash.digest('hex');
}

function validateCargo() {
  const version = workspaceVersion();
  for (const name of ['labcoat-cli', 'labcoat-core', 'labcoat-test']) {
    const pkg = cargoPackage(`crates/${name}/Cargo.toml`).packageBlock;
    if (!/^version\.workspace\s*=\s*true$/m.test(pkg)) fail(`${name} must inherit the workspace version`);
    const shouldPublish = name === 'labcoat-test';
    if (!new RegExp(`^publish\\s*=\\s*${shouldPublish}$`, 'm').test(pkg)) {
      fail(`${name} publish must be ${shouldPublish}`);
    }
  }
  for (const path of ['crates/isomer-core/Cargo.toml']) {
    if (!/^publish\s*=\s*false$/m.test(cargoPackage(path).packageBlock)) fail(`${path} must not be publishable`);
  }
  const template = read('crates/labcoat-cli/templates/default/Cargo.toml');
  if (!template.includes('labcoat-test = "={{LABCOAT_VERSION}}"')) fail('project template must use the CLI version placeholder');
  if (!template.includes('metashrew-support = { git = "https://github.com/kungfuflex/metashrew", tag = "v9.0.5-rc.8" }')) {
    fail('project template must use the same metashrew source as alkanes-rs');
  }
  if (template.includes('sandshrewmetaprotocols/metashrew')) fail('project template uses the legacy metashrew remote');
  const trigger = read('crates/labcoat-test/RELEASE_TRIGGER').trim();
  if (!sha256.test(trigger)) fail('labcoat-test release trigger must be a SHA-256 digest');
  if (trigger !== releaseTriggerDigest()) fail('labcoat-test release trigger is stale; run update-release-trigger.mjs');
  return version;
}

function validateRuntime() {
  if (manifest.schema !== 2) fail('runtime.json schema must be 2');
  const active = manifest.active_release;
  if (!active?.owner || !active?.repository || !active?.tag) fail('runtime.json has an incomplete active release');
  if (!(runtimeTag.test(active.tag) || (active.owner === 'jonatns' && active.repository === 'isomer' && active.tag === 'binaries-v0.1.3'))) {
    fail(`invalid active runtime tag: ${active.tag}`);
  }
  if (runtimeTag.test(active.tag)) {
    if (active.owner !== 'jonatns' || active.repository !== 'labcoat') {
      fail('promoted runtime releases must come from jonatns/labcoat');
    }
    if (active.checksums_asset !== 'checksums.json') {
      fail('promoted runtime releases must use checksums.json');
    }
  }
  const sources = Object.entries(manifest.sources ?? {});
  if (sources.length !== 3) fail('runtime.json must declare exactly three build sources');
  for (const [name, source] of sources) {
    if (!source.repository || !source.revision || !source.version) fail(`incomplete runtime source: ${name}`);
    if (['main', 'master', 'develop', 'trunk', 'HEAD'].includes(source.revision)) fail(`${name} uses moving ref ${source.revision}`);
    if (!gitCommit.test(source.revision) && !/v\d/.test(source.revision)) fail(`${name} revision is not an immutable commit or version tag`);
  }
  const expectedSources = {
    qubitcoin: 'e7f2f9d8844bdc7662030d98abb0544cc3e5a8da',
    'alkanes-wasm': '714843c416e2ab57352a33f05b8461cf3f540f5a',
    'esplorashrew-wasm': '7f7660908cdb54d12540ac6a8b337ef6a70e8057',
  };
  for (const [name, revision] of Object.entries(expectedSources)) {
    if (manifest.sources[name]?.revision !== revision) fail(`${name} must use the approved revision`);
  }
  for (const key of ['qubitcoind', 'alkanes_wasm', 'esplorashrew_wasm']) {
    const component = manifest.hosted?.[key];
    if (!component?.asset_pattern || !component?.version || !component?.size_bytes) fail(`incomplete hosted component: ${key}`);
    const hashes = Object.values(component.sha256 ?? {});
    if (hashes.length === 0 || hashes.some((hash) => !sha256.test(hash))) fail(`invalid hosted checksums: ${key}`);
  }
  if (manifest.external !== undefined) fail('runtime.json must not declare external runtime binaries');
  const platforms = Object.keys(manifest.hosted.qubitcoind.sha256).sort().join(',');
  if (platforms !== 'darwin-arm64,linux-x86_64') {
    fail('qubitcoind must declare the two supported native runtime platforms');
  }
  for (const key of ['alkanes_wasm', 'esplorashrew_wasm']) {
    if (Object.keys(manifest.hosted[key].sha256).join(',') !== 'all') {
      fail(`${key} must be platform independent`);
    }
  }
}

function validateActions() {
  for (const name of readdirSync(resolve(root, '.github/workflows'))) {
    if (!name.endsWith('.yml') && !name.endsWith('.yaml')) continue;
    const source = read(`.github/workflows/${name}`);
    for (const match of source.matchAll(/^\s*(?:-\s+)?uses:\s*([^\s#]+)/gm)) {
      const target = match[1];
      if (target.startsWith('./')) continue;
      if (!/@[0-9a-f]{40}$/.test(target)) fail(`${name} uses an action that is not pinned to a full commit SHA: ${target}`);
    }
  }
}

function main() {
  const [command = 'validate', value] = process.argv.slice(2);
  if (command === '--workspace-version') return console.log(workspaceVersion());
  if (command === '--runtime-source-digest') return console.log(sourceDigest());
  if (command === '--validate-cli-tag') {
    if (!cliTag.test(value ?? '')) fail(`invalid CLI tag: ${value ?? ''}`);
    if (value !== `cli-v${workspaceVersion()}`) fail(`${value} does not match workspace version ${workspaceVersion()}`);
    return;
  }
  if (command === '--validate-runtime-tag') {
    if (!runtimeTag.test(value ?? '')) fail(`invalid runtime tag: ${value ?? ''}`);
    return;
  }
  if (command === '--expect-runtime-source-digest') {
    if (sourceDigest() !== value) fail('runtime sources changed after the bundle was built; refusing promotion');
    return;
  }
  if (command !== 'validate') fail(`unknown command: ${command}`);
  const version = validateCargo();
  validateRuntime();
  validateActions();
  console.log(`release metadata valid (CLI ${version}, runtime sources ${sourceDigest()})`);
}

main();
