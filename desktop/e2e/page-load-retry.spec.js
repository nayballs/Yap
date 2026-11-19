// A page whose own code can't be fetched loads again (index.html's
// `__yapLoadFailed`, App.svelte's view import). 19 Nov 2026: one font
// stylesheet refused by the dev server as Yap started failed main.js's
// imports, so nothing ran and the main window stayed blank until a restart.
// A module script's error event fires when any file it imports can't be
// loaded; the page now loads again, at most twice a minute, then says "Yap
// couldn't show this window" with Reload. (More in never-blank.spec.js.)
// The entry is refused with CDP's Fetch (support/intercept.js: Playwright's
// `route` sometimes held nothing on these pages).
import { test, expect } from './support/fixtures.js';
import { intercept } from './support/intercept.js';

/** The page's own entry (a build's `/assets/index-….js`, or Vite's `/src/main.js`). */
const ENTRY = ['*/assets/index-*.js*', '*/src/main.js*'];

// Rust's watchdog (shell/page_watch.rs), which loads a page again that
// never says its view is in, kept out of these: they're the page's own part.
test.beforeEach(async ({ yap }) => {
  await yap.invoke('e2e_page_watch', { allowanceMs: 600_000, reset: true });
});
test.afterEach(async ({ yap }) => {
  await yap.invoke('e2e_page_watch', { allowanceMs: 0, reset: true }).catch(() => {});
});

test("the main window's code refused once: the page loads again and the app is there", async ({ main }) => {
  let refused = 0;
  let served = 0;
  const held = await intercept(main, ENTRY, () => {
    if (refused === 0) {
      refused += 1;
      return 'refuse';
    }
    served += 1;
    return 'continue';
  });
  try {
    const retried = main.waitForEvent('console', { predicate: (m) => /couldn't load; loading the page again/.test(m.text()) });
    await main.evaluate(() => sessionStorage.removeItem('yap.loadRetries'));
    await main.reload();
    await retried;
    // Loaded again by itself: the app is there.
    await expect(main.locator('.home[data-ready]')).toBeVisible({ timeout: 20_000 });
    expect(refused).toBe(1);
    expect(served).toBeGreaterThanOrEqual(1);
  } finally {
    await held.stop();
  }
});

test('a page that can never load stops trying after two loads a minute, and says so with Reload', async ({ main }) => {
  let asked = 0;
  const held = await intercept(main, ENTRY, () => {
    asked += 1;
    return 'refuse';
  });
  try {
    await main.evaluate(() => sessionStorage.removeItem('yap.loadRetries'));
    await main.reload();
    // The first load and two more, then it stays put: never a blank window.
    await expect.poll(() => asked, { timeout: 15_000 }).toBe(3);
    const message = main.getByRole('alert').filter({ hasText: /Yap couldn.t show this window/ });
    await expect(message).toBeVisible();
    await main.waitForTimeout(2_000);
    expect(asked).toBe(3);
    // Put right: Reload brings the app back.
    await held.stop();
    await message.getByRole('button', { name: 'Reload' }).click();
    await expect(main.locator('.home[data-ready]')).toBeVisible({ timeout: 20_000 });
    await expect(message).toHaveCount(0);
  } finally {
    await held.stop();
  }
});
