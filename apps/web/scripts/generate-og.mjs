import { access, mkdir, readFile, readdir, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import sharp from 'sharp';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = path.resolve(root, '../..');
const check = process.argv.includes('--check');
const brand = JSON.parse(await readFile(path.join(repoRoot, 'brand.json'), 'utf8'));

const escapeXml = (value) => value
  .replaceAll('&', '&amp;')
  .replaceAll('<', '&lt;')
  .replaceAll('>', '&gt;');

async function fontData(packageName, matcher) {
  const filesRoot = path.join(root, 'node_modules', packageName, 'files');
  const match = (await readdir(filesRoot)).find((file) => matcher.test(file));
  if (!match) throw new Error(`Could not find the required ${packageName} Latin font file`);
  return (await readFile(path.join(filesRoot, match))).toString('base64');
}

const sans = await fontData('@fontsource-variable/ibm-plex-sans', /latin-wght-normal\.woff2$/);
const mono = await fontData('@fontsource/ibm-plex-mono', /latin-600-normal\.woff2$/);
const headline = escapeXml(brand.socialHeadline);
const description = escapeXml(brand.description);
const domain = escapeXml(brand.domain);
const headlineParts = brand.socialHeadline.split(' to ');
const headlineLines = [headlineParts[0], `to ${headlineParts.slice(1).join(' to ')}`].map(escapeXml);
const descriptionWords = brand.description.split(' ');
const descriptionLines = descriptionWords.reduce((lines, word) => {
  const current = lines.at(-1);
  if (!current || `${current} ${word}`.length > 74) lines.push(word);
  else lines[lines.length - 1] = `${current} ${word}`;
  return lines;
}, []);

const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="630" viewBox="0 0 1200 630" fill="none">
  <style>
    @font-face { font-family: 'Plex Sans'; src: url(data:font/woff2;base64,${sans}) format('woff2'); font-weight: 100 700; }
    @font-face { font-family: 'Plex Mono'; src: url(data:font/woff2;base64,${mono}) format('woff2'); font-weight: 600; }
    .sans { font-family: 'Plex Sans', sans-serif; }
    .mono { font-family: 'Plex Mono', monospace; }
  </style>
  <rect width="1200" height="630" rx="28" fill="#0B0F0C"/>
  <defs>
    <pattern id="minor" width="20" height="20" patternUnits="userSpaceOnUse"><path d="M20 0H0V20" stroke="#E9F2E9" stroke-opacity=".035"/></pattern>
    <pattern id="major" width="80" height="80" patternUnits="userSpaceOnUse"><path d="M80 0H0V80" stroke="#B8FF65" stroke-opacity=".09"/></pattern>
    <radialGradient id="glow"><stop stop-color="#B8FF65" stop-opacity=".18"/><stop offset="1" stop-color="#B8FF65" stop-opacity="0"/></radialGradient>
  </defs>
  <rect width="1200" height="630" fill="url(#minor)"/><rect width="1200" height="630" fill="url(#major)"/>
  <circle cx="1010" cy="110" r="330" fill="url(#glow)"/>
  <path d="M0 505H115L150 470L202 520L255 455L318 505H445L490 478L540 505H1200" stroke="#B8FF65" stroke-opacity=".18" stroke-width="2"/>
  <g transform="translate(72 66)">
    <path d="M24 8h16M28 8v14L13 48a5.3 5.3 0 0 0 4.6 8h28.8a5.3 5.3 0 0 0 4.6-8L36 22V8" stroke="#F4F8F2" stroke-width="4" stroke-linecap="round" stroke-linejoin="round"/>
    <path d="m21 43 8-7 6 5 5-5 5 7" stroke="#B8FF65" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"/>
    <circle cx="34" cy="29" r="2.5" fill="#FFB347"/>
  </g>
  <text x="148" y="105" class="mono" fill="#F4F8F2" font-size="24" font-weight="600" letter-spacing="4">LABCOAT</text>
  <text x="72" y="220" class="sans" fill="#F4F8F2" font-size="72" font-weight="620" letter-spacing="-2.5"><tspan x="72">${headlineLines[0]}</tspan><tspan x="72" dy="78">${headlineLines[1]}</tspan></text>
  <text x="72" y="365" class="sans" fill="#AEB9AE" font-size="25" font-weight="400">${descriptionLines.map((line, index) => `<tspan x="72" dy="${index === 0 ? 0 : 36}">${escapeXml(line)}</tspan>`).join('')}</text>
  <g transform="translate(72 450)">
    <rect width="440" height="60" fill="#151C17" stroke="#354338"/>
    <text x="22" y="38" class="mono" fill="#B8FF65" font-size="18">$</text>
    <text x="50" y="38" class="mono" fill="#F4F8F2" font-size="18">labcoat trace &lt;txid&gt; --wait</text>
  </g>
  <text x="72" y="572" class="mono" fill="#B8FF65" font-size="17" letter-spacing="2">${domain}  /  ALKANES DEVELOPMENT</text>
  <g transform="translate(930 405)">
    <path d="M48 0h80M68 0v70L0 190h176L108 70V0" stroke="#F4F8F2" stroke-width="12" stroke-linecap="round" stroke-linejoin="round"/>
    <path d="m22 158 42-38 34 26 30-28 30 40" stroke="#B8FF65" stroke-width="10" stroke-linecap="round" stroke-linejoin="round"/>
  </g>
  <title>${headline}</title>
  <desc>${description}</desc>
</svg>
`;

const png = await sharp(Buffer.from(svg)).png({ compressionLevel: 9, palette: true }).toBuffer();
const svgPath = path.join(root, 'public/og.svg');
const pngPath = path.join(root, 'public/og.png');
await mkdir(path.join(root, 'public'), { recursive: true });

if (check) {
  for (const [target, expected] of [[svgPath, Buffer.from(svg)], [pngPath, png]]) {
    try {
      await access(target);
      const existing = await readFile(target);
      if (!existing.equals(expected)) throw new Error(`${path.relative(repoRoot, target)} is stale; run pnpm --dir apps/web generate:og`);
    } catch (error) {
      if (error instanceof Error && error.message.includes('is stale')) throw error;
      throw new Error(`${path.relative(repoRoot, target)} is missing; run pnpm --dir apps/web generate:og`);
    }
  }
} else {
  await writeFile(svgPath, svg);
  await writeFile(pngPath, png);
  console.log('Updated apps/web/public/og.svg and apps/web/public/og.png');
}
