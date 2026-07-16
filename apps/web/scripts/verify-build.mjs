import { access, readFile, readdir } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = path.resolve(root, '../..');
const dist = path.join(root, 'dist');
const required = [
  'index.html',
  'docs/index.html',
  'docs/getting-started/quickstart/index.html',
  'docs/index.md.txt',
  'llms.txt',
  'llms-full.txt',
  'reference/cli.json',
  'skill.md',
  'install',
  'robots.txt',
  'sitemap-index.xml',
  'og.png',
];

for (const file of required) await access(path.join(dist, file));

async function walk(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const nested = await Promise.all(entries.map((entry) => {
    const target = path.join(directory, entry.name);
    return entry.isDirectory() ? walk(target) : [target];
  }));
  return nested.flat();
}

const files = await walk(dist);
const htmlFiles = files.filter((file) => file.endsWith('.html'));
const broken = [];

async function exists(relative) {
  const clean = decodeURIComponent(relative.split(/[?#]/)[0]).replace(/^\//, '');
  const candidates = clean.endsWith('/')
    ? [path.join(clean, 'index.html')]
    : [clean, `${clean}.html`, path.join(clean, 'index.html')];
  for (const candidate of candidates) {
    try { await access(path.join(dist, candidate)); return true; } catch { /* try next */ }
  }
  return false;
}

for (const file of htmlFiles) {
  const html = await readFile(file, 'utf8');
  for (const match of html.matchAll(/(?:href|src)="([^"]+)"/g)) {
    const href = match[1];
    if (/^(?:https?:|mailto:|tel:|#|data:)/.test(href)) continue;
    const resolved = href.startsWith('/')
      ? href
      : `/${path.relative(dist, path.resolve(path.dirname(file), href))}`;
    if (!(await exists(resolved))) broken.push(`${path.relative(dist, file)} -> ${href}`);
  }
}

if (broken.length) {
  throw new Error(`Broken local references:\n${broken.join('\n')}`);
}

const repoVercel = JSON.parse(await readFile(path.join(repoRoot, 'vercel.json'), 'utf8'));
const appVercel = JSON.parse(await readFile(path.join(root, 'vercel.json'), 'utf8'));
const hasHostRedirect = (config) => config.redirects?.some((redirect) =>
  redirect.has?.some((condition) => condition.type === 'host'));

if (hasHostRedirect(repoVercel) || hasHostRedirect(appVercel)) {
  throw new Error('Host redirects belong in Vercel Domains, not vercel.json.');
}
if (JSON.stringify(repoVercel.headers) !== JSON.stringify(appVercel.headers)) {
  throw new Error('Repository-root and app-root Vercel headers are out of sync.');
}
if (repoVercel.outputDirectory !== 'apps/web/dist' || appVercel.outputDirectory !== 'dist') {
  throw new Error('Vercel output directories do not match their project roots.');
}

console.log(`Verified ${required.length} required outputs and ${htmlFiles.length} HTML files.`);
