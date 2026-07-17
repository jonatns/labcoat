import sitemap from '@astrojs/sitemap';
import starlight from '@astrojs/starlight';
import { defineConfig } from 'astro/config';
import brand from '../../brand.json';

export default defineConfig({
  site: 'https://labcoat.sh',
  output: 'static',
  trailingSlash: 'ignore',
  integrations: [
    starlight({
      title: brand.name,
      description: brand.metaDescription,
      favicon: '/favicon.svg',
      logo: {
        dark: './src/assets/labcoat-mark.svg',
        light: './src/assets/labcoat-mark-light.svg',
        alt: '',
      },
      customCss: ['./src/styles/global.css'],
      components: {
        Banner: './src/components/DocsBanner.astro',
      },
      social: [
        {
          icon: 'github',
          label: 'Labcoat on GitHub',
          href: 'https://github.com/jonatns/labcoat',
        },
      ],
      editLink: {
        baseUrl: 'https://github.com/jonatns/labcoat/edit/main/apps/web/',
      },
      lastUpdated: true,
      tableOfContents: { minHeadingLevel: 2, maxHeadingLevel: 3 },
      expressiveCode: {
        themes: ['github-light', 'vesper'],
      },
      sidebar: [
        {
          label: 'Start here',
          items: [
            { label: 'Overview', slug: 'docs' },
            { label: 'Installation', slug: 'docs/getting-started/installation' },
            { label: 'Quick start', slug: 'docs/getting-started/quickstart' },
          ],
        },
        {
          label: 'Build with Labcoat',
          items: [
            { label: 'Projects & configuration', slug: 'docs/projects-configuration' },
            { label: 'Devnet & wallets', slug: 'docs/devnet-wallets' },
            { label: 'Contracts', slug: 'docs/contracts' },
            { label: 'Automation & agents', slug: 'docs/automation' },
          ],
        },
        {
          label: 'Reference',
          items: [
            { label: 'CLI reference', slug: 'docs/reference/cli' },
            { label: 'Protocol', slug: 'docs/reference/protocol' },
            { label: 'Errors & recovery', slug: 'docs/reference/errors' },
            { label: 'Stability & releases', slug: 'docs/reference/stability' },
            { label: 'Migration', slug: 'docs/migration' },
          ],
        },
      ],
      head: [
        { tag: 'meta', attrs: { name: 'theme-color', content: '#0a0d0b' } },
        { tag: 'meta', attrs: { property: 'og:site_name', content: brand.name } },
        { tag: 'meta', attrs: { property: 'og:description', content: brand.metaDescription } },
        { tag: 'meta', attrs: { property: 'og:image', content: 'https://labcoat.sh/og.png' } },
        { tag: 'meta', attrs: { name: 'twitter:card', content: 'summary_large_image' } },
      ],
    }),
    sitemap({
      filter: (page) => !page.endsWith('.md.txt/'),
    }),
  ],
});
