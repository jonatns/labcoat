import sitemap from '@astrojs/sitemap';
import starlight from '@astrojs/starlight';
import { defineConfig } from 'astro/config';
import brand from '../../brand.json';
import rehypeScrollTables from './src/plugins/rehype-scroll-tables.mjs';

export default defineConfig({
  site: 'https://labcoat.sh',
  output: 'static',
  trailingSlash: 'ignore',
  markdown: {
    rehypePlugins: [rehypeScrollTables],
  },
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
      customCss: ['./src/styles/global.css', './src/styles/docs.css'],
      components: {
        Banner: './src/components/DocsBanner.astro',
        Head: './src/components/DocsHead.astro',
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
            { label: 'Start with an agent', slug: 'docs/getting-started/agent-onboarding' },
            { label: 'Installation', slug: 'docs/getting-started/installation' },
            { label: 'Quick start', slug: 'docs/getting-started/quickstart' },
            { label: 'Tutorial: your first contract', slug: 'docs/tutorials/first-contract' },
            { label: 'Tutorial: tokens & exchanges', slug: 'docs/tutorials/token-exchange' },
            { label: 'Tutorial: an upgradeable contract', slug: 'docs/tutorials/upgradeable-contract' },
            { label: 'Tutorial: a web app', slug: 'docs/tutorials/web-app' },
          ],
        },
        {
          label: 'Build with Labcoat',
          items: [
            { label: 'Projects & configuration', slug: 'docs/projects-configuration' },
            { label: 'Labcoat Network & wallets', slug: 'docs/labcoat-network' },
            { label: 'Contracts', slug: 'docs/contracts' },
            { label: 'Deployment scripts', slug: 'docs/deployments' },
            { label: 'Deploy to signet', slug: 'docs/deploy-to-signet' },
            { label: 'Atomic exchanges', slug: 'docs/exchanges' },
            { label: 'Web apps', slug: 'docs/web-apps' },
            { label: 'Wallet integrations', slug: 'docs/web-wallets' },
            { label: 'Deployment state', slug: 'docs/deployment-state' },
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
          ],
        },
      ],
      head: [
        { tag: 'meta', attrs: { name: 'theme-color', content: '#0a0d0b' } },
        { tag: 'meta', attrs: { property: 'og:site_name', content: brand.name } },
        { tag: 'meta', attrs: { property: 'og:description', content: brand.metaDescription } },
        { tag: 'meta', attrs: { property: 'og:image', content: 'https://labcoat.sh/og.png' } },
        { tag: 'meta', attrs: { property: 'og:image:width', content: '1200' } },
        { tag: 'meta', attrs: { property: 'og:image:height', content: '630' } },
        { tag: 'meta', attrs: { property: 'og:image:alt', content: `${brand.name}: ${brand.socialHeadline}` } },
        { tag: 'meta', attrs: { name: 'twitter:card', content: 'summary_large_image' } },
      ],
    }),
    sitemap({
      filter: (page) => !page.endsWith('.md.txt/'),
    }),
  ],
});
