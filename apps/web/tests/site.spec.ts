import AxeBuilder from '@axe-core/playwright';
import { expect, test } from '@playwright/test';

// Starlight's search button is named "Search" but also shows a "Ctrl K"
// shortcut hint, which axe 4.14's label-content-name-mismatch rule reports.
// The markup is Starlight's, not ours: skip only that rule on only that
// button so every other check still runs against it.
type AxeResults = Awaited<ReturnType<AxeBuilder['analyze']>>;

const starlightSearchButton = 'button[data-open-modal=""]';

function seriousViolations(results: AxeResults) {
  return results.violations
    .filter((violation) => ['serious', 'critical'].includes(violation.impact ?? ''))
    .map((violation) =>
      violation.id === 'label-content-name-mismatch'
        ? { ...violation, nodes: violation.nodes.filter((node) => node.target.join(' ') !== starlightSearchButton) }
        : violation,
    )
    .filter((violation) => violation.nodes.length > 0);
}

test('homepage leads with install, the quick start, and a keyboard-accessible demo', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByRole('heading', { level: 1, name: 'From Rust source to decoded trace.' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Copy install command' }).first()).toBeVisible();
  await expect(page.getByRole('link', { name: /Run the quick start/ })).toHaveAttribute('href', '/docs/getting-started/quickstart/');
  await expect(page.getByRole('link', { name: /View source/ })).toHaveAttribute('href', 'https://github.com/jonatns/labcoat');
  await expect(page.getByLabel('Stable release compatibility')).toHaveCount(0);
  for (const name of ['Docs', 'GitHub']) {
    await expect(page.getByRole('navigation', { name: 'Primary navigation' }).getByRole('link', { name, exact: true })).toBeVisible();
  }
  for (const id of ['network', 'systems', 'agents']) {
    await expect(page.locator(`#${id} h2`)).toBeVisible();
  }

  const tabs = page.getByRole('tab');
  await tabs.first().focus();
  await expect(tabs.first()).toBeFocused();
  await page.keyboard.press('ArrowRight');
  await expect(tabs.nth(1)).toHaveAttribute('aria-selected', 'true');
  await expect(page.getByRole('tabpanel')).toHaveCount(1);
  await expect(page.getByRole('tabpanel').locator('[data-terminal-command]')).toHaveText('labcoat test');
  const focusOutline = await tabs.nth(1).evaluate((element) => getComputedStyle(element).outlineStyle);
  expect(focusOutline).not.toBe('none');
  await page.keyboard.press('ArrowLeft');
  await page.keyboard.press('ArrowLeft');
  await expect(tabs.last()).toHaveAttribute('aria-selected', 'true');
});

test('theme choice is shared between the homepage and the docs', async ({ page }) => {
  await page.goto('/');
  const initial = await page.locator('html').getAttribute('data-theme');
  await page.getByRole('button', { name: 'Switch color theme' }).click();
  const selected = initial === 'light' ? 'dark' : 'light';
  await expect(page.locator('html')).toHaveAttribute('data-theme', selected);
  await page.reload();
  await expect(page.locator('html')).toHaveAttribute('data-theme', selected);
  await page.goto('/docs/');
  await expect(page.locator('html')).toHaveAttribute('data-theme', selected);
});

test('install command can be copied, or is selected when the clipboard is unavailable', async ({ page }) => {
  await page.addInitScript(() => {
    Object.defineProperty(navigator, 'clipboard', { value: { writeText: () => Promise.reject(new Error('denied')) } });
  });
  await page.goto('/');
  const button = page.getByRole('button', { name: 'Copy install command' }).first();
  await button.click();
  await expect(button).toContainText('Selected');
  expect(await page.evaluate(() => window.getSelection()?.toString())).toBe('curl -fsSL https://labcoat.sh/install | sh');
});

test('every page has no serious accessibility violations or horizontal overflow, in both themes', async ({ page, request }) => {
  test.setTimeout(180_000);
  const sitemap = await (await request.get('/sitemap-0.xml')).text();
  const routes = [...sitemap.matchAll(/<loc>([^<]+)<\/loc>/g)].map((match) => new URL(match[1]).pathname);
  expect(routes.length).toBeGreaterThan(10);
  for (const theme of ['dark', 'light']) {
    await page.addInitScript((value) => localStorage.setItem('starlight-theme', value), theme);
    for (const route of routes) {
      // networkidle: Expressive Code makes scrollable code blocks focusable after load.
      await page.goto(route, { waitUntil: 'networkidle' });
      await expect(page.locator('html'), `${route} theme`).toHaveAttribute('data-theme', theme);
      const results = await new AxeBuilder({ page }).analyze();
      expect(seriousViolations(results), `${theme} ${route}`).toEqual([]);
      const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
      expect(overflow, `${theme} ${route} horizontal overflow`).toBeLessThanOrEqual(1);
    }
  }
});

test('documentation and agent surfaces are published', async ({ page, request }) => {
  await page.goto('/docs/');
  await expect(page.getByRole('heading', { level: 1 })).toContainText('Labcoat documentation');
  await expect(page.locator('.sl-banner')).toContainText('These docs track the current main branch. Run labcoat docs --llm for the reference bundled with your installed version.');

  const docsLogos = page.locator('.site-title img');
  await expect(docsLogos).toHaveCount(2);
  await page.locator('html').evaluate((element) => { element.dataset.theme = 'dark'; });
  await expect(docsLogos.nth(0)).toBeVisible();
  await expect(docsLogos.nth(1)).toBeHidden();
  await page.locator('html').evaluate((element) => { element.dataset.theme = 'light'; });
  await expect(docsLogos.nth(0)).toBeHidden();
  await expect(docsLogos.nth(1)).toBeVisible();

  for (const route of ['/llms.txt', '/llms-full.txt', '/docs/index.md.txt', '/docs/labcoat-network/', '/docs/reference/stability/', '/reference/cli.json', '/skill.md', '/install', '/og.svg', '/og.png']) {
    const response = await request.get(route);
    expect(response.ok(), route).toBeTruthy();
  }
});

test('reduced motion turns off smooth scrolling', async ({ page }) => {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.goto('/');
  const behavior = await page.locator('html').evaluate((element) => getComputedStyle(element).scrollBehavior);
  expect(behavior).toBe('auto');
});

test('fonts are self-hosted', async ({ page }) => {
  const fontRequests: string[] = [];
  page.on('request', (request) => {
    if (request.resourceType() === 'font') fontRequests.push(request.url());
  });
  await page.goto('/');
  await page.evaluate(() => document.fonts.ready);
  expect(fontRequests.length).toBeGreaterThan(0);
  expect(fontRequests.every((url) => new URL(url).origin === 'http://127.0.0.1:4321')).toBeTruthy();
});

test('content remains readable when web fonts are blocked', async ({ page }) => {
  let blockedFonts = 0;
  await page.route(/\.(?:woff2?|ttf)(?:\?.*)?$/, (route) => route.abort());
  page.on('request', (request) => {
    if (request.resourceType() === 'font') blockedFonts += 1;
  });
  await page.goto('/');
  await expect(page.getByRole('heading', { level: 1 })).toBeVisible();
  expect(blockedFonts).toBeGreaterThan(0);
  const family = await page.locator('body').evaluate((element) => getComputedStyle(element).fontFamily);
  expect(family).toContain('system-ui');
});
