#!/usr/bin/env node

import { createHash } from 'node:crypto';
import { readFile, stat, writeFile } from 'node:fs/promises';
import path from 'node:path';
import process from 'node:process';

const repoRoot = path.resolve(import.meta.dirname, '../..');
const valueFor = (flag) => {
  const index = process.argv.indexOf(flag);
  return index >= 0 ? process.argv[index + 1] : undefined;
};

const version = valueFor('--version');
const directory = path.resolve(repoRoot, valueFor('--directory') ?? 'dist');
const output = path.resolve(directory, valueFor('--output') ?? 'runtime-manifest.json');
const sourcesPath = path.resolve(
  repoRoot,
  valueFor('--sources') ?? 'crates/labcoat-cli/runtime-sources.json',
);

if (!version || !/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(version)) {
  throw new Error('usage: build-runtime-manifest.mjs --version X.Y.Z [--directory DIR]');
}

const sourceConfig = JSON.parse(await readFile(sourcesPath, 'utf8'));
if (sourceConfig.schema !== 2 || Object.keys(sourceConfig.sources ?? {}).length !== 1) {
  throw new Error('runtime source configuration is invalid');
}

// Labcoat Network runs inside the CLI; its one runtime asset is the
// portable Alkanes indexer module.
const assets = {};
for (const name of ['alkanes.wasm']) {
  const file = path.join(directory, name);
  const bytes = await readFile(file);
  const metadata = await stat(file);
  assets[name] = {
    sha256: createHash('sha256').update(bytes).digest('hex'),
    size_bytes: metadata.size,
  };
}

const sourceDigest = createHash('sha256')
  .update(JSON.stringify({ sources: sourceConfig.sources }))
  .digest('hex');
const manifest = {
  schema: 2,
  labcoat_version: version,
  release_tag: `cli-v${version}`,
  source_digest: sourceDigest,
  sources: sourceConfig.sources,
  assets,
};
await writeFile(output, `${JSON.stringify(manifest, null, 2)}\n`);
