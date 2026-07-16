# Working on the Labcoat website

The site is a static Astro + Starlight app deployed at `https://labcoat.sh`.

- Product page: `src/pages/index.astro`
- Public documentation: `src/content/docs/docs/**/*.md`
- Shared visual system: `src/styles/global.css`
- Agent endpoints: `src/pages/*.txt.ts`, `src/pages/docs/*.md.txt.ts`
- Generated CLI artifacts: `src/generated/cli-reference.json` and
  `src/content/docs/docs/reference/cli.md`

Run `pnpm sync:reference` after changing the CLI reference or MCP tools. Do not
hand-edit generated reference files. Keep docs in plain Markdown where possible
so their raw `.md.txt` equivalents remain clean.

All interactions must work with a keyboard, retain visible focus, satisfy WCAG
2.2 AA contrast, and respect `prefers-reduced-motion`. Add colors through the
existing OKLCH tokens instead of one-off values. Avoid adding client frameworks,
remote imagery, or analytics without an explicit product decision.
