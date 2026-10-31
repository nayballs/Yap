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
    await mainOnScreen(yap);
    // Yap's notice policy starts fresh: no cooldowns or fatigue counts from
    // the last test, and the person counts as looking at the main window
    // while it's on screen (notices/notice.rs; a test that wants them in another
    // app calls `inAnotherApp`).
    await yap.invoke('e2e_notice_reset').catch(() => {});
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

/**
 * The person is in another app (the call, an editor): Yap's notices go to
 * the Yap bar instead of the main window's toasts (notices/notice.rs: one place).
 * A test run can't move the real focus; this says where it is. Back to
 * "looking at Yap" before every test.
 */
export async function inAnotherApp(yap) {
  await yap.invoke('e2e_focus', { focused: false });
}

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

/**
 * Open Settings (from the sidebar cog) at a page, by the start of its name
 * in the nav ("General", "AI apps", "Privacy & data", …).
 */
export async function openSettings(main, section) {
  const dialog = settingsDialog(main);
  if (!(await dialog.isVisible())) {
    await sidebar(main).getByRole('button', { name: /^Settings/ }).click();
    await expect(dialog).toBeVisible();
  }
  if (section) {
    // Labels can carry an attention count ("Speech-to-Text 1"); a name can
    // carry brackets ("AI apps (MCP)").
    const name = section.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    const item = dialog
      .getByRole('navigation', { name: 'Settings sections' })
      .getByRole('button', { name: new RegExp(`^${name}`) });
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

/**
 * Close every toast. Each round re-finds whichever toasts are up at that
 * moment and clicks the first Close with a short timeout: a toast can
 * expire (or a new one arrive) mid-way, and a click on a toast that's gone
 * would otherwise wait out the whole test timeout.
 */
export async function closeToasts(main) {
  const toasts = main.getByRole('status');
  await expect(async () => {
    const close = toasts.getByRole('button', { name: 'Close', exact: true }).first();
    if ((await close.count()) > 0) await close.click({ timeout: 1_000 });
    await expect(toasts).toHaveCount(0, { timeout: 500 });
  }).toPass({ timeout: 20_000 });
}

/**
 * The main window on screen (shown, not minimized), as the suite started it.
 * A busy desktop (Win+D, another app, another test run beside this one) can
 * minimize it behind the suite's back, and Yap decides where some prompts go
 * by whether the main window is on screen (`window_view` in
 * calls/meeting_detect.rs): minimized, a call prompt goes to a Windows
 * notification, which test mode never shows, instead of the in-app toast.
 */
export async function mainOnScreen(yap) {
  // Not `open_settings`: that also runs the updater's "window shown" hook
  // (a re-check), which updates.spec.js must stay in charge of.
  await yap.invoke('e2e_main_on_screen');
  await expect
    .poll(
      async () => {
        const [visible, minimized] = await Promise.all([
          yap.invoke('plugin:window|is_visible', { label: 'settings' }),
          yap.invoke('plugin:window|is_minimized', { label: 'settings' }),
        ]);
        return visible && !minimized;
      },
      { message: 'the main window to be on screen', timeout: 10_000 },
    )
    .toBe(true);
}

/**
 * The "Connect your calendar" dialog (ConnectCalendarDialog.svelte). Its
 * title follows the step ("Outlook Calendar", "Calendar connected"…), and
 * every one of them has "calendar" in it.
 */
export function connectDialog(main) {
  return main.getByRole('dialog', { name: /calendar/i });
}

/**
 * Chat's full model picker, the provider rail ("Switch the Chat model"):
 * the model chip's short menu, then "Add a provider or download a model…"
 * (with no AI model the chip opens the rail at once). Returns the rail.
 */
export async function openModelPicker(main) {
  await openView(main, 'Chat');
  await main.locator('.modelchip .chip').click();
  const picker = main.getByRole('dialog', { name: 'Switch the Chat model' });
  const menu = main.getByRole('menu', { name: 'Chat model' });
  await expect(picker.or(menu)).toBeVisible();
  if (await menu.isVisible()) await menu.getByRole('menuitem', { name: 'Add a provider or download a model…' }).click();
  await expect(picker).toBeVisible();
  return picker;
}

/** Back to a known state between tests: no dialog or toasts, Settings closed, Home showing. */
export async function resetUi(main) {
  const dialog = connectDialog(main);
  if (await dialog.isVisible()) {
    await dialog.getByRole('button', { name: 'Close', exact: true }).click();
    await expect(dialog).toBeHidden();
  }
  await closeToasts(main);
  await closeSettings(main);
  await openView(main, 'Home');
}

/**
 * Press the seeded dictation hotkey (F24) in a webview the way a keyboard
 * does. The main window's in-page fallback turns it into a dictation toggle.
 * (Playwright's key names stop at F12, so this talks CDP directly.)
 */
export async function pressHotkey(page) {
  await hotkeyEvents(page, ['rawKeyDown', 'keyUp']);
}

/** The seeded dictation hotkey going down (push-to-talk: held until `hotkeyUp`). */
export async function hotkeyDown(page) {
  await hotkeyEvents(page, ['rawKeyDown']);
}

/** The seeded dictation hotkey coming up. */
export async function hotkeyUp(page) {
  await hotkeyEvents(page, ['keyUp']);
}

async function hotkeyEvents(page, types) {
  const cdp = await page.context().newCDPSession(page);
  try {
    const key = { key: 'F24', code: 'F24', windowsVirtualKeyCode: 135, nativeVirtualKeyCode: 135 };
    for (const type of types) await cdp.send('Input.dispatchKeyEvent', { type, ...key });
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

/** Every value in `want` is in `now`: objects field by field (a saved value
 *  may carry fields of its own), arrays item by item, the rest exactly. */
function holds(now, want) {
  if (Array.isArray(want)) {
    return Array.isArray(now) && now.length === want.length && want.every((w, i) => holds(now[i], w));
  }
  if (want && typeof want === 'object') {
    return !!now && typeof now === 'object' && Object.entries(want).every(([k, v]) => holds(now[k], v));
  }
  return JSON.stringify(now) === JSON.stringify(want);
}

/**
 * Change config.json as another surface would (the Integrations view's
 * Local API switch does it this way), and tell Settings' copy
 * (`yap-config-patched`). Settings' copy also adopts any save on its own
 * (`yap-config-saved`), and its auto-save puts only its own changes onto
 * config.json as it is now — writing nothing when it has none (Settings.svelte
 * "Staying in step"). Wait out that auto-save (400 ms), then check the
 * config holds every patched value. A spec that went on at once raced it:
 * a late save of the last patch put back what this one changed (CI,
 * 2026-10-23, before Settings merged its saves).
 */
export async function setConfig(yap, main, patch) {
  const cfg = await yap.invoke('get_config');
  await yap.invoke('save_config', { cfg: { ...cfg, ...patch } });
  await main.evaluate((p) => window.dispatchEvent(new CustomEvent('yap-config-patched', { detail: p })), patch);
  await main.waitForTimeout(700);
  await expect
    .poll(async () => holds(await yap.invoke('get_config'), patch), {
      message: `the config to hold ${Object.keys(patch).join(', ')}`,
    })
    .toBe(true);
}
