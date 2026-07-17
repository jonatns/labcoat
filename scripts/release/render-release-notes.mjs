import { readFile, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const valueFor = (flag) => {
  const index = process.argv.indexOf(flag);
  return index >= 0 ? process.argv[index + 1] : undefined;
};

const version = valueFor('--version');
const tag = valueFor('--tag') ?? (version ? `cli-v${version}` : undefined);
const changelogPath = path.resolve(repoRoot, valueFor('--changelog') ?? 'CHANGELOG.md');
const outputPath = valueFor('--output');

if (!version || !tag) {
  throw new Error('usage: render-release-notes.mjs --version X.Y.Z [--tag cli-vX.Y.Z] [--changelog PATH] [--output PATH]');
}

const changelog = await readFile(changelogPath, 'utf8');
const escapedVersion = version.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
const sectionHeader = new RegExp(`^## \\[${escapedVersion}\\][^\\n]*\\n`, 'm').exec(changelog);
if (!sectionHeader) throw new Error(`CHANGELOG.md has no [${version}] release section`);
const sectionRemainder = changelog.slice(sectionHeader.index + sectionHeader[0].length);
const nextSection = sectionRemainder.search(/^## \[/m);
const releaseChanges = sectionRemainder.slice(0, nextSection < 0 ? undefined : nextSection).trim();
const compatibilityMatch = releaseChanges.match(/^### (?:Breaking changes|Compatibility|Removed|Deprecated)\s*\n([\s\S]*?)(?=^### |$(?![\s\S]))/im);
const compatibility = compatibilityMatch?.[1].trim()
  ?? 'Labcoat is pre-1.0. Review the command and behavior changes above before upgrading pinned automation.';
const maturity = 'Early-stage software for local Alkanes development. Interfaces may change before 1.0; mainnet deployment controls are not production-ready.';

const notes = `# Labcoat ${tag}

## User-facing changes

${releaseChanges}

## Compatibility and breaking changes

${compatibility}

After installation, compare \`labcoat --help\` and \`labcoat docs --llm\` with any pinned scripts or agent instructions.

## Install

\`\`\`sh
curl -fsSL https://labcoat.sh/install | sh -s -- ${version}
labcoat --version
\`\`\`

## Verify downloads

The installer requires \`sha256sum\` or \`shasum\` and verifies the published SHA-256 checksum automatically. To verify GitHub build provenance as well:

\`\`\`sh
gh release download ${tag} --repo jonatns/labcoat --pattern 'labcoat-*'
gh attestation verify ./labcoat-* --repo jonatns/labcoat
\`\`\`

## Known limitations

- ${maturity}
- Supported release binaries target macOS and Linux on arm64 and x86_64. Windows is unsupported.
- The website tracks the current main branch; \`labcoat docs --llm\` is the installed-version reference.
- The local gateway requires Node.js, and Labcoat downloads and orchestrates multiple local services.

Read the [full changelog](https://github.com/jonatns/labcoat/blob/${tag}/CHANGELOG.md).
`;

if (outputPath) {
  await writeFile(path.resolve(repoRoot, outputPath), notes);
} else {
  process.stdout.write(notes);
}
