#!/usr/bin/env node

import { mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { spawnSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import path from 'node:path';

const repoRoot = path.resolve(import.meta.dirname, '../..');
const directory = await mkdtemp(path.join(tmpdir(), 'labcoat-runtime-manifest-'));
for (const [name, contents] of [
  ['qubitcoind-darwin-arm64', 'darwin'],
  ['qubitcoind-linux-x86_64', 'linux'],
  ['alkanes.wasm', 'alkanes'],
  ['esplorashrew.wasm', 'esplora'],
]) {
  await writeFile(path.join(directory, name), contents);
}

const result = spawnSync(process.execPath, [
  'scripts/release/build-runtime-manifest.mjs',
  '--version', '1.2.3',
  '--directory', directory,
], { cwd: repoRoot, encoding: 'utf8' });
if (result.status !== 0) throw new Error(result.stderr || 'runtime manifest build failed');

const manifest = JSON.parse(await readFile(path.join(directory, 'runtime-manifest.json'), 'utf8'));
if (manifest.schema !== 1) throw new Error('unexpected manifest schema');
if (manifest.release_tag !== 'cli-v1.2.3') throw new Error('release tag does not match version');
if (!/^[0-9a-f]{64}$/.test(manifest.source_digest)) throw new Error('invalid source digest');
if (Object.keys(manifest.sources).length !== 3) throw new Error('runtime sources are incomplete');
if (!/^[0-9a-f]{40}$/.test(manifest.compatibility.qubitcoin_metashrew_revision)) {
  throw new Error('runtime compatibility pin is incomplete');
}
if (Object.keys(manifest.assets).length !== 4) throw new Error('runtime assets are incomplete');
if (manifest.assets['qubitcoind-linux-x86_64'].platform !== 'linux-x86_64') {
  throw new Error('native platform metadata is incorrect');
}
if (manifest.assets['alkanes.wasm'].platform !== null) {
  throw new Error('portable WASM asset unexpectedly has a platform');
}
for (const asset of Object.values(manifest.assets)) {
  if (!/^[0-9a-f]{64}$/.test(asset.sha256) || asset.size_bytes <= 0) {
    throw new Error('runtime asset integrity metadata is invalid');
  }
}

console.log('runtime manifest tests passed');
