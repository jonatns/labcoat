import AxeBuilder from '@axe-core/playwright';
import { expect, test } from '@playwright/test';

test('homepage navigation and interactive protocol runner are keyboard accessible', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByRole('heading', { level: 1, name: 'From Rust source to decoded trace.' })).toBeVisible();
  await expect(page.getByRole('link', { name: /Run the quick start/ })).toHaveAttribute('href', '/docs/getting-started/quickstart/');
  await expect(page.getByRole('link', { name: /View source/ })).toHaveAttribute('href', 'https://github.com/jonatns/labcoat');
  await expect(page.getByLabel('Stable release compatibility')).toContainText('cli-v0.1.0');
  for (const label of ['Scaffold', 'Test', 'Run the devnet', 'Deploy & inspect']) {
    await expect(page.getByRole('heading', { name: label, exact: true })).toBeVisible();
  }

  const tabs = page.getByRole('tab');
  await tabs.first().focus();
  await expect(tabs.first()).toBeFocused();
  await page.keyboard.press('ArrowRight');
  await expect(tabs.nth(1)).toHaveAttribute('aria-selected', 'true');
  await expect(page.locator('[data-runner-command]')).toContainText('labcoat test');
  const focusOutline = await tabs.nth(1).evaluate((element) => getComputedStyle(element).outlineStyle);
  expect(focusOutline).not.toBe('none');

  await page.getByRole('button', { name: 'Switch color theme' }).click();
  const selectedTheme = await page.locator('html').getAttribute('data-theme');
  expect(selectedTheme).toMatch(/light|dark/);
  await page.reload();
  await expect(page.locator('html')).toHaveAttribute('data-theme', selectedTheme ?? 'dark');
});

test('homepage has no serious accessibility violations or horizontal overflow', async ({ page }) => {
  for (const theme of ['dark', 'light']) {
    await page.addInitScript((value) => localStorage.setItem('labcoat-theme', value), theme);
    await page.goto('/');
    const results = await new AxeBuilder({ page }).analyze();
    expect(results.violations.filter((violation) => ['serious', 'critical'].includes(violation.impact ?? '')), theme).toEqual([]);
    const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
    expect(overflow, `${theme} horizontal overflow`).toBeLessThanOrEqual(1);
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

  const results = await new AxeBuilder({ page }).analyze();
  expect(results.violations.filter((violation) => ['serious', 'critical'].includes(violation.impact ?? ''))).toEqual([]);

  for (const route of ['/llms.txt', '/llms-full.txt', '/docs/index.md.txt', '/docs/reference/stability/', '/reference/cli.json', '/skill.md', '/install', '/og.svg', '/og.png']) {
    const response = await request.get(route);
    expect(response.ok(), route).toBeTruthy();
  }
});

test('reduced motion disables long animations', async ({ page }) => {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.goto('/');
  await page.getByRole('tab').nth(1).click();
  const duration = await page.locator('#runner-output').evaluate((element) => getComputedStyle(element).animationDuration);
  expect(Number.parseFloat(duration)).toBeLessThanOrEqual(0.001);
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
