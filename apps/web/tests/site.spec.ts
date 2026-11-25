import AxeBuilder from '@axe-core/playwright';
import { expect, test } from '@playwright/test';
import { readFileSync } from 'node:fs';

// The one prompt the site asks people to hand to an agent.
const agentPrompt = readFileSync(new URL('../src/agent-prompt.txt', import.meta.url), 'utf8').trim();
const normalize = (text: string) => text.replace(/\s+/g, ' ').trim();

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

test('homepage leads with the agent prompt, keeps the manual path, and has a keyboard-accessible demo', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByRole('heading', { level: 1, name: 'From Rust source to decoded trace.' })).toBeVisible();

  // Agent is the default way in; the installer is one tab away.
  const start = page.getByRole('tablist', { name: 'How to start' });
  await expect(start.getByRole('tab', { name: 'Agent' })).toHaveAttribute('aria-selected', 'true');
  const hero = page.locator('.hero');
  await expect(hero.getByRole('button', { name: 'Copy agent prompt' })).toBeVisible();
  await expect(hero.getByText('Paste this into Claude Code, Codex, or another coding agent with terminal access.')).toBeVisible();
  await expect(hero.getByRole('button', { name: 'Copy install command' })).toBeHidden();
  await expect(hero.getByRole('link', { name: /What your agent checks/ })).toHaveAttribute('href', '/docs/getting-started/agent-onboarding/');
  // Plain English, not a shell command.
  await expect(hero.locator('.agent-prompt .prompt')).toHaveCount(0);
  // The button is the action; the text is folded away until asked for.
  const promptText = hero.locator('.agent-prompt-text');
  await expect(promptText).toBeHidden();
  await hero.getByText('Read the prompt').focus();
  await page.keyboard.press('Enter');
  await expect(promptText).toBeVisible();
  await expect(promptText).toContainText('I authorize the official user-local installer');
  await page.keyboard.press('Enter');
  await expect(promptText).toBeHidden();

  await start.getByRole('tab', { name: 'Agent' }).focus();
  await page.keyboard.press('ArrowRight');
  await expect(start.getByRole('tab', { name: 'Manual' })).toBeFocused();
  await expect(start.getByRole('tab', { name: 'Manual' })).toHaveAttribute('aria-selected', 'true');
  await expect(hero.getByRole('button', { name: 'Copy install command' })).toBeVisible();
  await expect(hero.getByRole('button', { name: 'Copy agent prompt' })).toBeHidden();
  await page.keyboard.press('Home');
  await expect(hero.getByRole('button', { name: 'Copy agent prompt' })).toBeVisible();

  await expect(page.getByRole('link', { name: /Run the quick start/ })).toHaveAttribute('href', '/docs/getting-started/quickstart/');
  await expect(page.getByRole('link', { name: /View source/ })).toHaveAttribute('href', 'https://github.com/jonatns/labcoat');
  await expect(page.getByLabel('Stable release compatibility')).toHaveCount(0);
  for (const name of ['Docs', 'GitHub']) {
    await expect(page.getByRole('navigation', { name: 'Primary navigation' }).getByRole('link', { name, exact: true })).toBeVisible();
  }
  for (const id of ['network', 'systems', 'agents']) {
    await expect(page.locator(`#${id} h2`)).toBeVisible();
  }

  const demo = page.locator('.hero-demo');
  const tabs = demo.getByRole('tab');
  await tabs.first().focus();
  await expect(tabs.first()).toBeFocused();
  await page.keyboard.press('ArrowRight');
  await expect(tabs.nth(1)).toHaveAttribute('aria-selected', 'true');
  await expect(demo.getByRole('tabpanel')).toHaveCount(1);
  await expect(demo.getByRole('tabpanel').locator('[data-terminal-command]')).toHaveText('labcoat test');
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

test('the agent prompt is copied exactly, from the keyboard, and the copy is announced', async ({ page }) => {
  await page.addInitScript(() => {
    const copied: string[] = [];
    Object.defineProperty(window, '__copied', { value: copied });
    Object.defineProperty(navigator, 'clipboard', {
      value: { writeText: (text: string) => { copied.push(text); return Promise.resolve(); } },
    });
  });
  await page.goto('/');
  for (const region of ['.hero', '.closing']) {
    // The button's name is its text, which changes while it reports the
    // copy, so find it by where it is.
    const button = page.locator(`${region} .agent-prompt button`);
    await expect(button).toHaveAccessibleName('Copy agent prompt');
    await button.focus();
    await page.keyboard.press('Enter');
    await expect(button).toContainText('Copied');
    await expect(page.getByRole('status')).toHaveText('Copied to the clipboard.');
    await expect(button).toContainText('Copy agent prompt');
  }
  const copied = await page.evaluate(() => (window as unknown as { __copied: string[] }).__copied);
  // Byte for byte the canonical prompt: paragraphs, and no line breaks inside one.
  expect(copied).toEqual([agentPrompt, agentPrompt]);
  expect(agentPrompt.split('\n\n')).toHaveLength(3);
  expect(agentPrompt.split('\n\n').every((paragraph) => !paragraph.includes('\n'))).toBeTruthy();
});

test('the agent prompt is selected in full when the clipboard is unavailable', async ({ page }) => {
  await page.addInitScript(() => {
    Object.defineProperty(navigator, 'clipboard', { value: { writeText: () => Promise.reject(new Error('denied')) } });
  });
  await page.goto('/');
  const button = page.locator('.hero .agent-prompt button');
  await button.click();
  await expect(button).toContainText('Selected');
  // Selecting needs the text on screen, so the prompt unfolds.
  await expect(page.locator('.hero .agent-prompt-text')).toBeVisible();
  await expect(page.getByRole('status')).toContainText('The text is selected');
  const selected = await page.evaluate(() => window.getSelection()?.toString() ?? '');
  expect(normalize(selected)).toBe(normalize(agentPrompt));
});

test('the agent prompt wraps inside its card at every width', async ({ page }) => {
  await page.goto('/');
  for (const region of ['.hero', '.closing']) {
    await page.locator(`${region} .agent-prompt-details summary`).click();
    const text = page.locator(`${region} .agent-prompt-text`);
    await expect(text).toBeVisible();
    const overflow = await text.evaluate((element) => element.scrollWidth - element.clientWidth);
    expect(overflow, region).toBeLessThanOrEqual(1);
    const card = await page.locator(`${region} .agent-prompt`).boundingBox();
    const viewport = page.viewportSize();
    expect(card && viewport && card.x >= 0 && card.x + card.width <= viewport.width, region).toBeTruthy();
  }
});

test('the manual install command can be copied, or is selected when the clipboard is unavailable', async ({ page }) => {
  await page.addInitScript(() => {
    Object.defineProperty(navigator, 'clipboard', { value: { writeText: () => Promise.reject(new Error('denied')) } });
  });
  await page.goto('/');
  await page.getByRole('tab', { name: 'Manual' }).click();
  const button = page.getByRole('button', { name: 'Copy install command' });
  await button.click();
  await expect(button).toContainText('Selected');
  expect(await page.evaluate(() => window.getSelection()?.toString())).toBe('curl -fsSL https://labcoat.sh/install | sh');
});

test('the agent guide and its plain-text forms serve the same prompt the homepage copies', async ({ page, request }) => {
  const served = await (await request.get('/agent-prompt.txt')).text();
  expect(served.trim()).toBe(agentPrompt);

  await page.goto('/docs/getting-started/agent-onboarding/');
  await expect(page.getByRole('heading', { level: 1, name: 'Start with an agent' })).toBeVisible();
  // In the sidebar's Start here group (a drawer on small screens).
  await expect(page.locator('nav.sidebar a[href="/docs/getting-started/agent-onboarding/"]')).toHaveCount(1);

  // The prompt sends agents to the plain-text page: fetched with curl, the
  // HTML page is mostly navigation markup.
  const preflight = agentPrompt.match(/https:\/\/labcoat\.sh(\/\S+) for preflight/)?.[1];
  expect(preflight).toBe('/docs/getting-started/agent-onboarding.md.txt');
  const raw = await (await request.get('/docs/getting-started/agent-onboarding.md.txt')).text();
  expect(raw).not.toContain('<html');
  // The preflight stops at init and hands over to the project's own files.
  expect(raw).toContain('labcoat init');
  expect(raw).toContain('AGENTS.md');
  expect(raw).not.toContain('labcoat deploy');

  for (const route of ['/llms.txt', '/skill.md']) {
    expect(await (await request.get(route)).text(), route).toContain('/docs/getting-started/agent-onboarding.md.txt');
  }
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

  for (const route of ['/llms.txt', '/llms-full.txt', '/agent-prompt.txt', '/docs/getting-started/agent-onboarding.md.txt', '/docs/index.md.txt', '/docs/labcoat-network/', '/docs/reference/stability/', '/reference/cli.json', '/skill.md', '/install', '/og.svg', '/og.png']) {
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
