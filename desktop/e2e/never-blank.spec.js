// A Yap window is never left blank (19 Nov 2026; docs/architecture/frontend.md
// "A window is never left blank", app-shell.md "The windows' pages watched").
// - The page's own part (index.html's head script, App.svelte's
//   `<svelte:boundary>`): a view that throws as it starts, or main.js
//   throwing before it mounts the app, loads the page again once, then
//   says "Yap couldn't show this window" with Reload; never on the Yap bar,
//   a transparent window, which only loads again.
// - Rust's watchdog (shell/page_watch.rs): a page that never says its view
//   is in loads again, at most 3 times in 5 minutes; so does one whose
//   renderer crashed. `e2e_page_watch` reports it and shortens its waits.
// A view that throws is its own chunk served with its component swapped for
// one that throws (support/intercept.js): no hook in the app. A crash is
// CDP's `Page.crash`.
import fs from 'node:fs';
import { test, expect } from './support/fixtures.js';
import { intercept } from './support/intercept.js';

// Its own Yap: these reload, refuse and crash its windows.
test.use({ yapOptions: { name: 'never-blank' } });

/** The page's own entry script (and Vite's other `index-…` chunks). */
const ENTRY = ['*/assets/index-*.js*'];
/** The window's page itself (a request for it is the `Document`; `\?` a question mark). */
const DOCUMENT = ['http://tauri.localhost/', 'http://tauri.localhost/\\?*', 'http://tauri.localhost/index.html*'];

/** Rust's watchdog, asked from the onboarding window (hidden, never reloaded here). */
const watch = (yap, args = {}) =>
  yap.onboarding.evaluate(([a]) => window.__TAURI_INTERNALS__.invoke('e2e_page_watch', a), [args]);
const windowWatch = async (yap, label) => (await watch(yap)).windows[label];
const logText = (yap) => fs.readFileSync(yap.logPath, 'utf8');
const fallback = (page) => page.getByRole('alert').filter({ hasText: /Yap couldn.t show this window/ });
const home = (main) => main.locator('.home[data-ready]');

/** The page's own reloads forgotten, so it loads itself again as it would. */
async function fresh(page) {
  await page.evaluate(() => {
    sessionStorage.removeItem('yap.loadRetries');
    sessionStorage.removeItem('yap.viewRetries');
  });
}

/**
 * A window's view (`ControlPanel`, `Overlay`) that throws as it renders:
 * its chunk as the app built it, but for the component the window takes
 * from it, which throws. That's its `default` export, or, from a chunk
 * shared with other views, the view module's namespace under the name
 * Rollup gave it (the entry's `import("./ControlPanel-….js").then(m =>
 * m.bj)`), whose `default` it is. Returns its URL and that body.
 */
async function viewThatThrows(page, view) {
  const { url, name, body } = await page.evaluate(async (view) => {
    const entry = document.querySelector('script[type="module"][src]').src;
    const code = await (await fetch(entry)).text();
    const m = code.match(new RegExp(`import\\("\\./(${view}-[\\w-]+\\.js)"\\)(?:\\.then\\(\\(?([\\w$]+)\\)?=>\\2\\.([\\w$]+)\\))?`));
    if (!m) throw new Error(`the entry doesn't import ${view}`);
    const url = new URL(`./${m[1]}`, entry).href;
    return { url, name: m[3] || 'default', body: await (await fetch(url)).text() };
  }, view);
  const at = body.lastIndexOf('export{');
  const binding = new RegExp(`[\\w$]+ as ${name.replace(/\$/g, '\\$')}(?=[,}])`);
  expect(binding.test(body.slice(at)), `${view}'s chunk exports it as ${name}`).toBe(true);
  const throws = "function View(){throw new TypeError('a view that throws (test)')}";
  const standIn = name === 'default' ? throws : `Object.freeze({__proto__:null,default:${throws}})`;
  const swapped = body.slice(0, at) + body.slice(at).replace(binding, `__yapThrows as ${name}`) + `\nvar __yapThrows=${standIn};\n`;
  return { url, body: swapped };
}

/** The uncaught errors a test made on purpose: not the fixture's to fail it on. */
function madeOnPurpose(yap, pattern) {
  const mine = yap.pageErrors.slice(yap.seen.pageErrors);
  for (const e of mine) expect(e.message).toMatch(pattern);
  yap.seen.pageErrors = yap.pageErrors.length;
}

/** The page's loads that found its code refused, as it logged them, since `from`. */
const refusedLoads = (yap, from, label = 'settings') =>
  (logText(yap).slice(from).match(new RegExp(`\\[web\\] window ${label}: the app couldn't load`, 'g')) || []).length;

let held = [];
/** `intercept`, let go after the test whatever happens. */
async function hold(page, patterns, answer) {
  const h = await intercept(page, patterns, answer);
  held.push(h);
  return h;
}

test.afterEach(async ({ yap }) => {
  for (const h of held) await h.stop();
  held = [];
  await watch(yap, { allowanceMs: 0, reset: true }).catch(() => {});
});

test('a view that throws as it starts: loaded again once, then the message, and Reload brings it back', async ({ yap, main, shot }) => {
  // Rust's watchdog out of the way: the page's own part.
  await watch(yap, { allowanceMs: 600_000, reset: true });
  const logFrom = logText(yap).length;
  const since = () => logText(yap).slice(logFrom);
  const view = await viewThatThrows(main, 'ControlPanel');
  let served = 0;
  const swap = await hold(main, [view.url], () => {
    served += 1;
    return { body: view.body };
  });
  await fresh(main);
  await main.reload();
  await expect(fallback(main)).toBeVisible({ timeout: 20_000 });
  // The first load and one more (once a minute), then the message.
  expect(served).toBe(2);
  await expect(fallback(main).getByRole('button', { name: 'Reload' })).toBeVisible();
  await expect(fallback(main).getByRole('button', { name: 'Close' })).toBeVisible();
  await expect(home(main)).toHaveCount(0);
  await shot(main, '01-a-view-that-throws');
  expect((await windowWatch(yap, 'settings')).stage).toBe('trouble');
  // Yap's log says what happened: the window, why, where in the code; once
  // loaded again, then the message. Never what the error said.
  await expect
    .poll(since)
    .toMatch(/WARN.*\[web\] window settings: the settings window's view failed \(TypeError at ControlPanel-[\w-]+\.js:\d+\); loading the page again/);
  await expect.poll(since).toMatch(/WARN.*\[web\] window settings: the settings window's view failed \(TypeError[^)]*\); showing Reload/);
  expect(since().match(/settings window's view failed/g)).toHaveLength(2);
  expect(since()).not.toContain('a view that throws (test)');

  // Put right, Reload brings the window back.
  await swap.stop();
  await fallback(main).getByRole('button', { name: 'Reload' }).click();
  await expect(home(main)).toBeVisible({ timeout: 20_000 });
  await expect(fallback(main)).toHaveCount(0);
  await expect.poll(async () => (await windowWatch(yap, 'settings')).stage).toBe('mounted');
  // (Still uncaught as far as the suite can tell: the boundary hides no bug.)
  madeOnPurpose(yap, /a view that throws \(test\)/);
});

test("the message follows the dark theme even when none of the app's code loaded", async ({ yap, main, shot }) => {
  await watch(yap, { allowanceMs: 600_000, reset: true });
  const refuse = await hold(main, ENTRY, () => 'refuse');
  await main.evaluate(() => localStorage.setItem('yap.theme', 'dark'));
  try {
    await fresh(main);
    await main.reload();
    await expect(fallback(main)).toBeVisible({ timeout: 20_000 });
    const colours = await fallback(main).evaluate((el) => ({
      bg: getComputedStyle(el).backgroundColor,
      fg: getComputedStyle(el.querySelector('h1')).color,
      button: getComputedStyle(el.querySelector('.yf-reload')).backgroundColor,
    }));
    // app.css's dark `--yap-bg`, `--yap-fg` and `--yap-ink`.
    expect(colours).toEqual({ bg: 'rgb(22, 20, 16)', fg: 'rgb(242, 239, 232)', button: 'rgb(236, 232, 223)' });
    await shot(main, '02-dark');
  } finally {
    await main.evaluate(() => localStorage.setItem('yap.theme', 'light'));
    await refuse.stop();
  }
  await fallback(main).getByRole('button', { name: 'Reload' }).click();
  await expect(home(main)).toBeVisible({ timeout: 20_000 });
});

test('main.js throwing before it mounts the app: loaded again once, then the message', async ({ yap, main }) => {
  await watch(yap, { allowanceMs: 600_000, reset: true });
  const entry = await main.evaluate(() => document.querySelector('script[type="module"][src]').src);
  let served = 0;
  const swap = await hold(main, [entry], () => {
    served += 1;
    return { body: "throw new RangeError('main.js throws (test)');" };
  });
  await fresh(main);
  await main.reload();
  await expect(fallback(main)).toBeVisible({ timeout: 20_000 });
  expect(served).toBe(2);
  await expect.poll(() => logText(yap)).toMatch(/WARN.*\[web\] window settings: the app failed \(RangeError[^)]*\); showing Reload/);
  await swap.stop();
  await fallback(main).getByRole('button', { name: 'Reload' }).click();
  await expect(home(main)).toBeVisible({ timeout: 20_000 });
  madeOnPurpose(yap, /main\.js throws \(test\)/);
});

test('the watchdog: a page whose code is refused on every load comes back once it can load', async ({ yap, main }) => {
  // Rust's waits cut to 1.5 s for the test (10 s: a page that said its view failed).
  await watch(yap, { allowanceMs: 1_500, reset: true });
  const logFrom = logText(yap).length;
  let allow = false;
  await hold(main, ENTRY, () => (allow ? 'continue' : 'refuse'));
  await fresh(main);
  await main.reload();
  // The first load, the page's own two, then its message; then Rust's reload.
  await expect.poll(async () => (await windowWatch(yap, 'settings')).reloads, { timeout: 20_000 }).toBeGreaterThanOrEqual(1);
  expect(refusedLoads(yap, logFrom)).toBeGreaterThanOrEqual(3);
  allow = true;
  // Rust's next reload finds the code there.
  await expect(home(main)).toBeVisible({ timeout: 20_000 });
  const w = await windowWatch(yap, 'settings');
  expect(w.stage).toBe('mounted');
  expect(w.gaveUp).toBe(false);
  expect(logText(yap).slice(logFrom)).toMatch(/WARN.*Reloading a window's page \(1 of 3 in 5 minutes\) window=settings reason="?its view failed/);
});

test('the watchdog gives up at its cap: three reloads in 5 minutes, then the message stays', async ({ yap, main }) => {
  await watch(yap, { allowanceMs: 1_500, reset: true });
  const logFrom = logText(yap).length;
  const refuse = await hold(main, ENTRY, () => 'refuse');
  await fresh(main);
  await main.reload();
  await expect.poll(async () => (await windowWatch(yap, 'settings')).gaveUp, { timeout: 30_000 }).toBe(true);
  // The first load, the page's own two, Rust's three: each one refused.
  expect((await windowWatch(yap, 'settings')).reloads).toBe(3);
  await expect.poll(() => refusedLoads(yap, logFrom)).toBe(6);
  await main.waitForTimeout(4_000);
  expect(refusedLoads(yap, logFrom)).toBe(6);
  await expect(fallback(main)).toBeVisible();
  expect(logText(yap).slice(logFrom)).toMatch(/WARN.*Not reloading the window's page again: 3 reloads in 5 minutes already window=settings/);
  // The person's Reload, once it can load, brings it back.
  await refuse.stop();
  await fallback(main).getByRole('button', { name: 'Reload' }).click();
  await expect(home(main)).toBeVisible({ timeout: 20_000 });
});

test("the watchdog: a window whose page can't be fetched (WebView2's error page) loads again once it can", async ({ yap, main }) => {
  await watch(yap, { allowanceMs: 1_500, reset: true });
  const logFrom = logText(yap).length;
  let refused = 0;
  let allow = false;
  await hold(main, DOCUMENT, ({ resourceType }) => {
    if (allow || resourceType !== 'Document') return 'continue';
    refused += 1;
    return 'refuse';
  });
  // No script of the page's runs: only Rust can bring it back.
  await main.reload().catch(() => {});
  await expect.poll(() => refused, { timeout: 20_000 }).toBeGreaterThanOrEqual(2);
  allow = true;
  await expect(home(main)).toBeVisible({ timeout: 20_000 });
  const w = await windowWatch(yap, 'settings');
  expect(w.stage).toBe('mounted');
  expect(w.reloads).toBeGreaterThanOrEqual(1);
  expect(logText(yap).slice(logFrom)).toMatch(/WARN.*A window's page couldn't load \(WebView2's error page\) window=settings/);
});

test("the watchdog: a load that hangs before any content (the page's own retry, here) is loaded again", async ({ yap, main }) => {
  await watch(yap, { allowanceMs: 1_500, reset: true });
  const logFrom = logText(yap).length;
  let entries = 0;
  let hung = 0;
  // The entry refused once, so the page loads itself again; that load's
  // page is never answered (no content, no error page: nothing of the page
  // runs). Rust's reload goes through.
  await hold(main, ENTRY, () => (entries++ === 0 ? 'refuse' : 'continue'));
  await hold(main, DOCUMENT, ({ resourceType }) => {
    if (resourceType !== 'Document' || entries === 0 || hung > 0) return 'continue';
    hung += 1;
    return new Promise(() => {});
  });
  await fresh(main);
  await main.reload();
  await expect.poll(() => hung, { timeout: 15_000 }).toBe(1);
  await expect(home(main)).toBeVisible({ timeout: 20_000 });
  expect((await windowWatch(yap, 'settings')).reloads).toBe(1);
  expect(logText(yap).slice(logFrom)).toMatch(/WARN.*Reloading a window's page \(1 of 3 in 5 minutes\) window=settings reason="?its page never started/);
});

test('a window whose renderer crashes loads again by itself', async ({ yap, main }) => {
  await watch(yap, { allowanceMs: 0, reset: true });
  const logFrom = logText(yap).length;
  const since = () => logText(yap).slice(logFrom);
  const cdp = await main.context().newCDPSession(main);
  const crashed = main.waitForEvent('crash');
  cdp.send('Page.crash').catch(() => {});
  await crashed;
  // Every window whose renderer went (WebView2 may share one between
  // windows) is loaded again and comes back, without a hand on it.
  await expect
    .poll(
      () => {
        const stopped = [...since().matchAll(/A window's page stopped: its renderer exited window=([\w-]+)/g)].map((m) => m[1]);
        const back = new Set([...since().matchAll(/A window's page is back after a reload window=([\w-]+)/g)].map((m) => m[1]));
        return stopped.includes('settings') && stopped.every((l) => back.has(l));
      },
      { timeout: 30_000 },
    )
    .toBe(true);
  // Playwright is done with a crashed page: connect again.
  await yap.reattach();
  await expect(home(yap.main)).toBeVisible({ timeout: 20_000 });
  const w = await windowWatch(yap, 'settings');
  expect(w.crashes).toBe(1);
  expect(w.reloads).toBe(1);
  expect(w.stage).toBe('mounted');
});

test('the Yap bar: a view that throws never paints a box, and the bar comes back', async ({ yap }) => {
  const bar = yap.overlay;
  await expect(bar.getByLabel('Yap bar')).toBeVisible();
  await watch(yap, { allowanceMs: 1_500, reset: true });
  const logFrom = logText(yap).length;
  const view = await viewThatThrows(bar, 'Overlay');
  let served = 0;
  let allow = false;
  await hold(bar, [view.url], () => {
    if (allow) return 'continue';
    served += 1;
    return { body: view.body };
  });
  await fresh(bar);
  await bar.reload();
  // The page loads itself again once, then leaves it to Rust, which loads it again.
  await expect.poll(async () => (await windowWatch(yap, 'overlay')).reloads, { timeout: 20_000 }).toBeGreaterThanOrEqual(1);
  // Never a box: no message, nothing drawn, and click-through.
  await expect
    .poll(() =>
      bar
        .evaluate(() => ({
          message: !!document.getElementById('yap-fallback'),
          html: getComputedStyle(document.documentElement).backgroundColor,
          body: getComputedStyle(document.body).backgroundColor,
          drawn: document.getElementById('app').childElementCount,
        }))
        .catch(() => null),
    )
    .toEqual({ message: false, html: 'rgba(0, 0, 0, 0)', body: 'rgba(0, 0, 0, 0)', drawn: 0 });
  expect(served).toBeGreaterThanOrEqual(2);
  expect((await yap.invoke('bar_debug')).interactive).toBe(false);
  expect(logText(yap).slice(logFrom)).toMatch(/WARN.*\[web\] window overlay: the overlay window's view failed \(TypeError[^)]*\); leaving it to Yap/);
  allow = true;
  await expect(bar.getByLabel('Yap bar')).toBeVisible({ timeout: 20_000 });
  await expect.poll(async () => (await windowWatch(yap, 'overlay')).stage).toBe('mounted');
  madeOnPurpose(yap, /a view that throws \(test\)/);
});

test("the Yap bar never shows WebView2's error page: a blank page at once, then the bar again", async ({ yap }) => {
  const bar = yap.overlay;
  await watch(yap, { allowanceMs: 1_500, reset: true });
  let refused = 0;
  let allow = false;
  await hold(bar, DOCUMENT, ({ resourceType }) => {
    if (allow || resourceType !== 'Document') return 'continue';
    refused += 1;
    return 'refuse';
  });
  await bar.reload().catch(() => {});
  // WebView2's "can't reach this page" would be an opaque box over the screen.
  await expect.poll(() => bar.url(), { timeout: 10_000 }).toBe('about:blank');
  expect(refused).toBeGreaterThanOrEqual(1);
  allow = true;
  await expect(bar.getByLabel('Yap bar')).toBeVisible({ timeout: 20_000 });
  await expect.poll(async () => (await windowWatch(yap, 'overlay')).stage).toBe('mounted');
});
