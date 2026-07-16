#!/usr/bin/env node

import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { execFileSync } from 'node:child_process';

const root = resolve(import.meta.dirname, '../..');
const trigger = 'crates/labcoat-test/RELEASE_TRIGGER';
const files = execFileSync(
  'git',
  ['ls-files', '-z', 'crates/labcoat-cli', 'crates/labcoat-core', 'crates/labcoat-test'],
  { cwd: root },
)
  .toString()
  .split('\0')
  .filter((path) => path && path !== trigger)
  .sort();

const hash = createHash('sha256');
for (const path of files) {
  hash.update(path);
  hash.update('\0');
  hash.update(readFileSync(resolve(root, path)));
  hash.update('\0');
}

writeFileSync(resolve(root, trigger), `${hash.digest('hex')}\n`);
