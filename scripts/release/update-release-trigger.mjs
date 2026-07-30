#!/usr/bin/env node

import { createHash } from 'node:crypto';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { execFileSync } from 'node:child_process';
import process from 'node:process';

const root = resolve(import.meta.dirname, '../..');
const trigger = 'crates/labcoat-test/RELEASE_TRIGGER';

function releaseTriggerDigest() {
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

const [command, ...rest] = process.argv.slice(2);
if (rest.length > 0 || !['--print', '--write'].includes(command)) {
  throw new Error('usage: update-release-trigger.mjs --print|--write');
}

const digest = releaseTriggerDigest();
if (command === '--print') {
  console.log(digest);
} else {
  writeFileSync(resolve(root, trigger), `${digest}\n`);
}
