import type { APIRoute } from 'astro';
import brand from '../../../../brand.json';

const modules = import.meta.glob<string>('../content/docs/docs/**/*.md', {
  query: '?raw',
  import: 'default',
  eager: true,
});

function stripFrontmatter(markdown: string) {
  return markdown.replace(/^---\r?\n[\s\S]*?\r?\n---\r?\n/, '');
}

const body = Object.entries(modules)
  .sort(([left], [right]) => left.localeCompare(right))
  .map(([path, markdown]) => {
    const slug = path.replace('../content/docs/docs/', '').replace(/\.md$/, '');
    return `<!-- source: https://labcoat.sh/docs/${slug === 'index' ? '' : `${slug}/`} -->\n\n${stripFrontmatter(markdown).trim()}`;
  })
  .join('\n\n---\n\n');

export const GET: APIRoute = () =>
  new Response(`# ${brand.name} — ${brand.tagline}\n\n${brand.description}\n\n${brand.maturityNotice}\n\n${brand.docsChannelNotice}\n\n${body}\n`, {
    headers: { 'Content-Type': 'text/plain; charset=utf-8' },
  });
