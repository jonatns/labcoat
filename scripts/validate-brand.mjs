import { readFile, readdir } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const read = async (relativePath) => readFile(path.join(repoRoot, relativePath), 'utf8');
const failures = [];
const assert = (condition, message) => {
  if (!condition) failures.push(message);
};

const brand = JSON.parse(await read('brand.json'));
const requiredStrings = {
  tagline: 'From Rust source to decoded trace.',
  description: 'Labcoat is the Rust-native CLI for building, testing, and operating Alkanes smart contracts with a complete local Bitcoin devnet.',
  metaTitle: 'Labcoat — Rust-native Alkanes development',
  metaDescription: 'Build, test, deploy, simulate, and trace Alkanes smart contracts with one CLI and a managed local Bitcoin devnet.',
  maturityNotice: 'Early-stage software for local Alkanes development. Interfaces may change before 1.0; mainnet deployment controls are not production-ready.',
};

for (const [key, expected] of Object.entries(requiredStrings)) {
  assert(brand[key] === expected, `brand.json ${key} must equal the approved canonical string`);
}

assert(brand.name === 'Labcoat', 'brand.json name must be Labcoat');
assert(brand.interfaceName === 'Labcoat CLI', 'brand.json interfaceName must be Labcoat CLI');
assert(brand.executable === 'labcoat', 'brand.json executable must be labcoat');
assert(brand.stableRelease?.tag === 'cli-v0.1.0', 'brand.json must identify the current stable release');

const directConsumers = [
  'apps/web/src/pages/index.astro',
  'apps/web/src/pages/llms.txt.ts',
  'apps/web/astro.config.ts',
  'apps/web/scripts/generate-og.mjs',
];
for (const file of directConsumers) {
  const source = await read(file);
  assert(source.includes('brand.json'), `${file} must consume brand.json directly`);
}

const readme = await read('README.md');
const rootPackage = JSON.parse(await read('package.json'));
const webPackage = JSON.parse(await read('apps/web/package.json'));
const cliCargo = await read('crates/labcoat-cli/Cargo.toml');
const cliMain = await read('crates/labcoat-cli/src/main.rs');
const skill = await read('skills/SKILL.md');
const templateSkill = await read('crates/labcoat-cli/templates/default/SKILL.md');
const docsOverview = await read('apps/web/src/content/docs/docs/index.md');
const normalize = (value) => value.replace(/^> ?/gm, '').replace(/\s+/g, ' ').trim();

assert(readme.includes(brand.tagline), 'README.md must contain the canonical tagline');
assert(readme.includes(brand.description), 'README.md must contain the canonical description');
assert(readme.includes(brand.maturityNotice), 'README.md must contain the canonical maturity notice');
assert(rootPackage.description === brand.description, 'package.json description must match brand.json');
assert(webPackage.description === brand.description, 'apps/web/package.json description must match brand.json');
assert(cliCargo.includes(`description = "${brand.description}"`), 'CLI Cargo description must match brand.json');
assert(cliMain.includes(`about = "${brand.description}"`), 'CLI about text must match brand.json');
assert(skill.includes(`description: ${brand.description}`), 'canonical skill description must match brand.json');
assert(templateSkill.includes(`description: ${brand.description}`), 'project template skill description must match brand.json');
assert(normalize(docsOverview).includes(brand.description), 'docs overview must contain the canonical description');
assert(normalize(docsOverview).includes(brand.maturityNotice), 'docs overview must contain the canonical maturity notice');

const compatibilityFiles = [
  'README.md',
  'apps/web/src/content/docs/docs/getting-started/installation.md',
  'apps/web/src/content/docs/docs/reference/stability.md',
];
for (const file of compatibilityFiles) {
  const source = await read(file);
  for (const command of ['labcoat contract new', 'labcoat compile', 'labcoat new', 'labcoat build']) {
    assert(source.includes(command), `${file} must explain stable/main command compatibility (${command})`);
  }
}

for (const link of ['SECURITY.md', 'CONTRIBUTING.md']) {
  assert(readme.includes(link), `README.md must link ${link}`);
}

const webSourceRoot = path.join(repoRoot, 'apps/web/src');
const webSources = (await readdir(webSourceRoot, { recursive: true }))
  .filter((file) => /\.(?:astro|ts|md|json)$/.test(file))
  .map((file) => path.join('apps/web/src', file));
const bannedSurfaces = [
  'README.md',
  'skills/SKILL.md',
  'crates/labcoat-cli/templates/default/SKILL.md',
  ...webSources,
];
const bannedClaims = [
  [/\bone binary\b/i, 'one binary'],
  [/\bfull stack\b/i, 'full stack'],
  [/\bfull Alkanes (?:loop|stack)\b/i, 'full Alkanes loop/stack'],
  [/\bstay synchronized\b/i, 'stay synchronized'],
  [/\bagent[- ]native\b/i, 'agent native'],
  [/\bship\b/i, 'ship'],
];
for (const file of bannedSurfaces) {
  const source = await read(file);
  for (const [pattern, label] of bannedClaims) {
    assert(!pattern.test(source), `${file} contains banned or overstated phrase: ${label}`);
  }
}

if (failures.length > 0) {
  console.error(failures.map((failure) => `- ${failure}`).join('\n'));
  process.exit(1);
}

console.log('Brand platform validation passed.');
