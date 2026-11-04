// Home's first paint (frontend.md, "Home's first paint"; 2026-10-24).
//
// As the main window starts it asks Rust for ~60 things — 50 listeners, the
// settings four times, the installed models twice — and Home used to ask for
// its own data (history, stats, settings, models: four calls, sync ones
// holding the window's thread) behind all of them, so it drew last: 64
// calls before Home showed. Now Home's data comes in one call
// (`home_snapshot`, off the window's thread) asked for first, and reads
// several parts of the window ask for at once go once (`ipcShared.js`).
//
// Measured from a reload of the main window, every IPC call seen at the
// page's `fetch` to http://ipc.localhost/ (Tauri's IPC on Windows): how many
// were answered before Home had its data (`.home[data-ready]`) — what it
// waited behind. The guard is on that count, not the milliseconds (a busy CI
// runner's clock says little); the time is printed and attached.
import { test, expect } from './support/fixtures.js';

/** Before the page's own scripts: count the IPC calls, and note when Home is ready. */
function watchIpc() {
  const log = (window.__ipcLog = []);
  const realFetch = window.fetch.bind(window);
  window.fetch = (input, init) => {
    const url = typeof input === 'string' ? input : input?.url || '';
    if (url.startsWith('http://ipc.localhost/')) {
      const e = { cmd: decodeURIComponent(url.slice('http://ipc.localhost/'.length)), at: performance.now(), done: null };
      log.push(e);
      const p = realFetch(input, init);
      const done = () => (e.done = performance.now());
      p.then(done, done);
      return p;
    }
    return realFetch(input, init);
  };
  const mark = () => {
    if (!window.__homeReadyAt && document.querySelector('.home[data-ready]')) window.__homeReadyAt = performance.now();
  };
  new MutationObserver(mark).observe(document, { subtree: true, childList: true, attributes: true, attributeFilter: ['data-ready'] });
}

test("Home's data comes first, in one call, not behind the window's start-up", async ({ yap, main, shot }, testInfo) => {
  await main.addInitScript(watchIpc);
  const runs = [];
  for (let i = 0; i < 3; i++) {
    await main.reload();
    await expect(main.locator('.home[data-ready]')).toBeVisible();
    // Let the rest of the start-up finish, for the totals.
    await main.waitForTimeout(800);
    runs.push(
      await main.evaluate(() => {
        const ready = window.__homeReadyAt;
        // The calls answered before Home had its data: what it waited behind.
        const before = window.__ipcLog.filter((e) => e.done !== null && e.done < ready);
        return {
          readyMs: Math.round(ready),
          before: before.length,
          total: window.__ipcLog.length,
          listeners: window.__ipcLog.filter((e) => e.cmd === 'plugin:event|listen').length,
          homeAt: window.__ipcLog.findIndex((e) => e.cmd === 'home_snapshot'),
          counts: window.__ipcLog.reduce((m, e) => ((m[e.cmd] = (m[e.cmd] || 0) + 1), m), {}),
        };
      })
    );
  }
  const last = runs[runs.length - 1];
  const summary = runs.map((r) => `ready ${r.readyMs} ms, behind ${r.before} answered calls (of ${r.total})`).join('; ');
  console.log(`Home's first paint: ${summary}`);
  await testInfo.attach('ipc.json', { body: JSON.stringify(runs, null, 2), contentType: 'application/json' });

  // Home's own call goes out among the very first, and only once.
  expect(last.homeAt).toBeGreaterThanOrEqual(0);
  expect(last.homeAt).toBeLessThan(5);
  expect(last.counts.home_snapshot).toBe(1);
  // Home no longer asks for these itself, and reads asked at once go once:
  // the settings 3 times (Appearance, Settings, the model list; was 4), the
  // installed models once (was 2).
  for (const old of ['get_history', 'get_stats']) expect(last.counts[old] || 0).toBe(0);
  expect(last.counts.get_config || 0).toBeLessThanOrEqual(3);
  expect(last.counts.installed_models || 0).toBeLessThanOrEqual(1);
  // Home draws after a handful of calls, not ~64.
  for (const r of runs) expect(r.before).toBeLessThanOrEqual(12);
  await shot(main, '01-home-ready');
});
