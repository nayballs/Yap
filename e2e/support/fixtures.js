// Playwright fixtures for the Yap e2e suite. Every spec file imports `test`
// and `expect` from here.
//
// - `yap` (one per worker): a fresh, isolated Yap instance (support/yap.js).
//   A spec gets its own instance by overriding `yapOptions` — see
//   updates.spec.js. After a failed test Playwright starts a new worker, so
//   the next test gets a fresh instance too.
// - `main`: the main window ("settings" webview = the ControlPanel), reset to
//   Home before each test. Fails the test if any webview threw meanwhile.
// - `shot(page, name)`: a named screenshot in test-results/app/screenshots/
//   <spec>/<name>.png, also attached to the HTML report.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect } from '@playwright/test';
import { launchYap, SCREENSHOTS } from './yap.js';

export { expect };

export const test = base.extend({
  // { name, config, env } for launchYap. Specs override it with test.extend.
  yapOptions: [{ name: 'app' }, { scope: 'worker', option: true }],

  yap: [
    async ({ yapOptions }, use, workerInfo) => {
      const app = await launchYap({ ...yapOptions, name: `${yapOptions.name}-w${workerInfo.workerIndex}` });
      // How far each error list has been reported (errors belong to the test
      // that was running, or to the first test for those from startup).
      app.seen = { pageErrors: 0, consoleErrors: 0 };
      await use(app);
      await app.stop();
    },
    { scope: 'worker', timeout: 90_000 },
  ],

  main: async ({ yap }, use, testInfo) => {
    const context = yap.browser.contexts()[0];
    const tracing = await context.tracing
      .start({ title: testInfo.title, screenshots: true, snapshots: true })
      .then(() => true)
      .catch(() => false);
    await resetUi(yap.main);

    await use(yap.main);

    if (tracing) {
      const failed = testInfo.status !== testInfo.expectedStatus;
      await context.tracing
        .stop(failed ? { path: testInfo.outputPath('trace.zip') } : undefined)
        .catch(() => {});
    }
    const fresh = (list) => {
      const items = yap[list].slice(yap.seen[list]);
      yap.seen[list] = yap[list].length;
      return items.map((e) => `[${e.label}] ${e.message}`).join('\n');
    };
    // A console error (Yap's frontend never logs one itself; the browser does,
    // e.g. for a failed request) doesn't fail the test: it's printed and
    // attached to the report.
    const consoleErrors = fresh('consoleErrors');
    if (consoleErrors) {
      console.warn(`console errors during "${testInfo.title}":\n${consoleErrors}`);
      await testInfo.attach('console-errors.txt', { body: consoleErrors, contentType: 'text/plain' });
    }
    const pageErrors = fresh('pageErrors');
    if (pageErrors) throw new Error(`Uncaught error(s) in Yap's webviews:\n${pageErrors}`);
  },

  shot: async ({}, use, testInfo) => {
    const spec = path.basename(testInfo.file).replace(/\.spec\.js$/, '');
    await use(async (page, name) => {
      const file = path.join(SCREENSHOTS, spec, `${name}.png`);
      fs.mkdirSync(path.dirname(file), { recursive: true });
      await page.screenshot({ path: file, scale: 'css', animations: 'disabled' });
      await testInfo.attach(name, { path: file, contentType: 'image/png' });
    });
  },
});

/** The sidebar of the main window. */
export function sidebar(main) {
  return main.getByRole('navigation', { name: 'Main' });
}

/** Switch the main window to a sidebar view ("Home", "Notes", …). */
export async function openView(main, label) {
  const item = sidebar(main).getByRole('button', { name: label, exact: true });
  await item.click();
  await expect(item).toHaveAttribute('aria-current', 'page');
}

/** The Settings modal. */
export function settingsDialog(main) {
  return main.getByRole('dialog', { name: 'Settings' });
}

/** Open Settings (from the sidebar cog) at a section ("General", "About", …). */
export async function openSettings(main, section) {
  const dialog = settingsDialog(main);
  if (!(await dialog.isVisible())) {
    await sidebar(main).getByRole('button', { name: /^Settings/ }).click();
    await expect(dialog).toBeVisible();
  }
  if (section) {
    // Labels can carry an attention count ("Speech-to-Text 1").
    const item = dialog
      .getByRole('navigation', { name: 'Settings sections' })
      .getByRole('button', { name: new RegExp(`^${section}`) });
    await item.click();
    await expect(item).toHaveAttribute('aria-current', 'page');
  }
  return dialog;
}

export async function closeSettings(main) {
  const dialog = settingsDialog(main);
  if (await dialog.isVisible()) {
    await dialog.getByRole('button', { name: 'Close settings' }).click();
    await expect(dialog).toBeHidden();
  }
}

/** Back to a known state between tests: no toasts, Settings closed, Home showing. */
export async function resetUi(main) {
  const toasts = main.getByRole('status');
  for (const close of await toasts.getByRole('button', { name: 'Close', exact: true }).all()) {
    await close.click().catch(() => {}); // it may be on its way out already
  }
  await expect(toasts).toHaveCount(0);
  await closeSettings(main);
  await openView(main, 'Home');
}

/**
 * Press the seeded dictation hotkey (F24) in a webview the way a keyboard
 * does. The main window's in-page fallback turns it into a dictation toggle.
 * (Playwright's key names stop at F12, so this talks CDP directly.)
 */
export async function pressHotkey(page) {
  const cdp = await page.context().newCDPSession(page);
  try {
    const key = { key: 'F24', code: 'F24', windowsVirtualKeyCode: 135, nativeVirtualKeyCode: 135 };
    await cdp.send('Input.dispatchKeyEvent', { type: 'rawKeyDown', ...key });
    await cdp.send('Input.dispatchKeyEvent', { type: 'keyUp', ...key });
  } finally {
    await cdp.detach().catch(() => {});
  }
}

/** Poll a JSON store in the portable data dir until `check` passes. */
export async function expectStore(yap, file, check, message) {
  await expect
    .poll(() => {
      try {
        return check(yap.readJson(file));
      } catch {
        return false;
      }
    }, { message: message ?? `${file} to be updated`, timeout: 10_000 })
    .toBe(true);
}
