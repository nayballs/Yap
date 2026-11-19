// A page whose own code can't be fetched loads again (index.html's
// `__yapLoadFailed`, App.svelte's view import). 19 Nov 2026: one font
// stylesheet refused by the dev server as Yap started failed main.js's
// imports, so nothing ran and the main window stayed blank until a restart.
// A module script's error event fires when any file it imports can't be
// loaded; the page now loads again, at most twice a minute.
import { test, expect } from './support/fixtures.js';

/** The page's own entry (a build's `/assets/index-….js`, or Vite's `/src/main.js`). */
const ENTRY = /\/(assets\/index-[^/]*\.js|src\/main\.js)(\?|$)/;

test("the main window's code refused once: the page loads again and the app is there", async ({ main }) => {
  let refused = 0;
  let served = 0;
  await main.route(ENTRY, (route) => {
    if (refused === 0) {
      refused += 1;
      return route.abort('connectionrefused');
    }
    served += 1;
    return route.continue();
  });
  const retried = main.waitForEvent('console', { predicate: (m) => /couldn't load; loading the page again/.test(m.text()) });
  await main.evaluate(() => sessionStorage.removeItem('yap.loadRetries'));
  await main.reload();
  await retried;
  // Loaded again by itself: the app is there.
  await expect(main.locator('.home[data-ready]')).toBeVisible({ timeout: 20_000 });
  expect(refused).toBe(1);
  expect(served).toBeGreaterThanOrEqual(1);
  await main.unroute(ENTRY);
});

test('a page that can never load stops trying after two loads a minute', async ({ main }) => {
  let asked = 0;
  await main.route(ENTRY, (route) => {
    asked += 1;
    return route.abort('connectionrefused');
  });
  await main.evaluate(() => sessionStorage.removeItem('yap.loadRetries'));
  await main.reload();
  // The first load and two more, then it stays put.
  await expect.poll(() => asked, { timeout: 15_000 }).toBe(3);
  await main.waitForTimeout(2_000);
  expect(asked).toBe(3);
  // Put right: a reload by hand brings the app back.
  await main.unroute(ENTRY);
  await main.evaluate(() => sessionStorage.removeItem('yap.loadRetries'));
  await main.reload();
  await expect(main.locator('.home[data-ready]')).toBeVisible({ timeout: 20_000 });
});
