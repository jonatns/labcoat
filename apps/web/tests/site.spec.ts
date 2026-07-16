import AxeBuilder from '@axe-core/playwright';
import { expect, test } from '@playwright/test';

test('homepage navigation and interactive protocol runner are keyboard accessible', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByRole('heading', { level: 1 })).toContainText('The full Alkanes loop');
  const tabs = page.getByRole('tab');
  await tabs.first().focus();
  await page.keyboard.press('ArrowRight');
  await expect(tabs.nth(1)).toHaveAttribute('aria-selected', 'true');
  await expect(page.locator('[data-runner-command]')).toContainText('labcoat test');

  await page.getByRole('button', { name: 'Switch color theme' }).click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', /light|dark/);
  await page.reload();
  await expect(page.locator('html')).toHaveAttribute('data-theme', /light|dark/);
});

test('homepage has no serious accessibility violations or horizontal overflow', async ({ page }) => {
  await page.goto('/');
  const results = await new AxeBuilder({ page }).analyze();
  expect(results.violations.filter((violation) => ['serious', 'critical'].includes(violation.impact ?? ''))).toEqual([]);
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  expect(overflow).toBeLessThanOrEqual(1);
});

test('documentation and agent surfaces are published', async ({ page, request }) => {
  await page.goto('/docs/');
  await expect(page.getByRole('heading', { level: 1 })).toContainText('Labcoat documentation');
  const results = await new AxeBuilder({ page }).analyze();
  expect(results.violations.filter((violation) => ['serious', 'critical'].includes(violation.impact ?? ''))).toEqual([]);

  for (const route of ['/llms.txt', '/llms-full.txt', '/docs/index.md.txt', '/reference/cli.json', '/skill.md', '/install']) {
    const response = await request.get(route);
    expect(response.ok(), route).toBeTruthy();
  }
});

test('reduced motion disables long animations', async ({ page }) => {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.goto('/');
  const duration = await page.locator('.hero').evaluate((element) => getComputedStyle(element).animationDuration);
  expect(Number.parseFloat(duration)).toBeLessThanOrEqual(0.001);
});
