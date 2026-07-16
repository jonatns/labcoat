import type { APIRoute } from 'astro';

const modules = import.meta.glob<string>('../../content/docs/docs/**/*.md', {
  query: '?raw',
  import: 'default',
  eager: true,
});

function stripFrontmatter(markdown: string) {
  return markdown.replace(/^---\r?\n[\s\S]*?\r?\n---\r?\n/, '');
}

export function getStaticPaths() {
  return Object.entries(modules).map(([path, markdown]) => {
    const slug = path.replace('../../content/docs/docs/', '').replace(/\.md$/, '');
    return {
      params: { slug },
      props: { markdown: stripFrontmatter(markdown), slug },
    };
  });
}

export const GET: APIRoute = ({ props }) => {
  const { markdown, slug } = props as { markdown: string; slug: string };
  const source = `<!-- canonical: https://labcoat.sh/docs/${slug === 'index' ? '' : `${slug}/`} -->\n\n`;
  return new Response(`${source}${markdown.trim()}\n`, {
    headers: { 'Content-Type': 'text/plain; charset=utf-8' },
  });
};
