#!/usr/bin/env node

import { mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { spawnSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import path from 'node:path';

const repoRoot = path.resolve(import.meta.dirname, '../..');
const directory = await mkdtemp(path.join(tmpdir(), 'labcoat-runtime-manifest-'));
await writeFile(path.join(directory, 'alkanes.wasm'), 'alkanes');

const result = spawnSync(process.execPath, [
  'scripts/release/build-runtime-manifest.mjs',
  '--version', '1.2.3',
  '--directory', directory,
], { cwd: repoRoot, encoding: 'utf8' });
if (result.status !== 0) throw new Error(result.stderr || 'runtime manifest build failed');

const manifest = JSON.parse(await readFile(path.join(directory, 'runtime-manifest.json'), 'utf8'));
if (manifest.schema !== 2) throw new Error('unexpected manifest schema');
if (manifest.release_tag !== 'cli-v1.2.3') throw new Error('release tag does not match version');
if (!/^[0-9a-f]{64}$/.test(manifest.source_digest)) throw new Error('invalid source digest');
if (Object.keys(manifest.sources).join() !== 'alkanes-wasm') {
  throw new Error('the runtime has exactly one source, the Alkanes indexer');
}
if (Object.keys(manifest.assets).join() !== 'alkanes.wasm') {
  throw new Error('the runtime has exactly one asset, alkanes.wasm');
}
const asset = manifest.assets['alkanes.wasm'];
if (!/^[0-9a-f]{64}$/.test(asset.sha256) || asset.size_bytes !== 7) {
  throw new Error('runtime asset integrity metadata is invalid');
}

console.log('runtime manifest tests passed');
