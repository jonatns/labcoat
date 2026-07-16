# Working on the Labcoat website

The site is a static Astro + Starlight app deployed at `https://labcoat.sh`.
Vercel's project Root Directory is `apps/web`; its output directory is `dist`
as declared in this directory's `vercel.json`. The repository-root
`vercel.json` is the CLI/repo-root fallback, so keep shared headers aligned
between both files. Configure apex and `www` redirects only in Vercel Domains;
do not add host redirects here because they can conflict with domain-level
redirects and loop static assets.

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
