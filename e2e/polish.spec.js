// The 2026-10-22 polish pass, as a brand-new user meets Yap: onboarding that
// shows its main button without scrolling and ends on Yap's window, its
// shortcut picker (combos, the Copilot key), the Copilot key anywhere a
// shortcut is set, Home and Insights in people's words ("1 dictation",
// "Teams" not "ms-teams"), Home in a narrow window, one title style across
// the views, Chat saying what to do when its AI can't be reached, and the
// hidden windows' webviews asked to keep less in memory.
import net from 'node:net';
import {
  test as base,
  expect,
  openView,
  openSettings,
  closeSettings,
  settingsDialog,
  expectStore,
  mainOnScreen,
  pressHotkey,
} from './support/fixtures.js';
import { COPILOT_KEY, isCopilotKey, hotkeyParts, formatHotkeySpec, fillHotkey } from '../src/lib/core/hotkeys.js';
import { appLabel } from '../src/lib/core/appNames.js';
import { aiError } from '../src/lib/core/aiErrors.js';

/** A local port nothing listens on: the "AI server that isn't running". */
function closedPort() {
  return new Promise((resolve, reject) => {
    const srv = net.createServer();
    srv.on('error', reject);
    srv.listen(0, '127.0.0.1', () => {
      const { port } = srv.address();
      srv.close(() => resolve(port));
    });
  });
}

const now = Math.floor(Date.now() / 1000);
const ONE = { ts: now - 90, raw: 'see you at ten', text: 'See you at ten.', model: 'parakeet-tdt-0.6b-v3', app: 'ms-teams.exe' };

const test = base.extend({
  deadPort: [async ({}, use) => use(await closedPort()), { scope: 'worker' }],
  yapOptions: [
    async ({ deadPort }, use) => {
      await use({
        name: 'polish',
        data: { 'history.json': [ONE] },
        config: {
          llmScopes: {
            chat: { enabled: true, provider: 'custom', baseUrl: `http://127.0.0.1:${deadPort}/v1`, model: 'local-model' },
          },
        },
      });
    },
    { scope: 'worker' },
  ],
});

test('the Copilot key, aiErrors and app names on their own', () => {
  // Win + Shift + F23, however it's written, is one key called Copilot.
  expect(COPILOT_KEY).toBe('kb:shift+win+134');
  expect(isCopilotKey('kb:win+shift+134')).toBe(true);
  expect(isCopilotKey('kb:shift+win+134')).toBe(true);
  expect(isCopilotKey('kb:134')).toBe(false);
  expect(isCopilotKey('kb:ctrl+shift+win+134')).toBe(false);
  expect(hotkeyParts(COPILOT_KEY)).toEqual(['Copilot']);
  expect(formatHotkeySpec(COPILOT_KEY)).toBe('Copilot');
  expect(fillHotkey('Hold {key} while you talk', COPILOT_KEY)).toBe('Hold the Copilot key while you talk');
  // Other keys as before.
  expect(formatHotkeySpec('kb:alt+win+77')).toBe('Win + Alt + M');
  expect(hotkeyParts('kb:135')).toEqual(['F24']);

  // An AI that can't be reached, said so a person knows what to do.
  const local = aiError('request failed: error sending request for url (http://127.0.0.1:11434/v1/chat/completions)');
  expect(local).toEqual({
    text: "Couldn't reach the AI model on this PC (127.0.0.1:11434). Check that it's running (Ollama, LM Studio…), then try again.",
    settings: true,
    known: true,
  });
  expect(aiError('request failed: error sending request for url (https://api.groq.com/openai/v1/chat/completions)').text).toBe(
    "Couldn't reach api.groq.com. Check your internet connection, then try again.",
  );
  expect(aiError('HTTP 401: {"error":{"message":"Invalid API Key"}}').text).toMatch(/didn't accept the API key/);
  expect(aiError('HTTP 404: {"error":{"message":"The model `x` does not exist"}}').settings).toBe(true);
  expect(aiError('HTTP 503: upstream busy').text).toMatch(/problem on its side/);
  expect(aiError('No groq API key — add one in Settings → Language Models')).toMatchObject({ settings: true, known: true });
  // Anything else stays as it was, in the copyable error box.
  expect(aiError('something odd')).toEqual({ text: 'something odd', settings: false, known: false });

  // Apps by the names people know.
  expect(appLabel('ms-teams.exe')).toBe('Teams');
  expect(appLabel('WINWORD.EXE')).toBe('Word');
  expect(appLabel('msedge.exe')).toBe('Edge');
  expect(appLabel('Code.exe')).toBe('VS Code');
  expect(appLabel('myapp.exe')).toBe('Myapp');
  expect(appLabel('')).toBe('');
});

test("the hidden windows' webviews keep less in memory", async ({ yap }) => {
  // shell/webview_memory.rs: off screen → WebView2's low memory target; the Yap
  // bar is left alone.
  await expect
    .poll(() => yap.invoke('e2e_webview_memory'), { timeout: 20_000 })
    .toMatchObject({ settings: 'normal', onboarding: 'low', notepad: 'low' });
  expect(await yap.invoke('e2e_webview_memory')).not.toHaveProperty('overlay');
});

test('Home and Insights in plain words, Upload names its model', async ({ main, shot }) => {
  // One dictation, not "1 dictations"; the app by its name.
  await expect(main.locator('.statfoot')).toContainText('1 dictation, counted on this PC.');
  await openView(main, 'Insights');
  await expect(main.locator('.appname').first()).toHaveText('Teams');
  await expect(main.locator('.card').filter({ hasText: /^1\s*dictation$/ })).toHaveCount(1);
  await shot(main, '01-insights-app-names');
  await openView(main, 'Upload');
  await expect(main.getByText('(transcribes with Parakeet V3)', { exact: false })).toBeVisible();

  // One title style: the serif display face, as Meetings and Settings.
  for (const view of ['Insights', 'Upload', 'Dictionary', 'Integrations', 'Meetings']) {
    await openView(main, view);
    const family = await main.getByRole('heading', { level: 1 }).first().evaluate((h) => getComputedStyle(h).fontFamily);
    expect(family, view).toMatch(/EB Garamond/);
  }
});

test("Integrations folds the local API's endpoint list away for developers", async ({ main, shot }) => {
  await openView(main, 'Integrations');
  const endpoints = main.locator('details.endpoints');
  await expect(endpoints).not.toHaveAttribute('open');
  await expect(endpoints.locator('.route').first()).toBeHidden();
  await endpoints.locator('summary').click();
  await expect(endpoints.locator('.route').first()).toBeVisible();
  await shot(main, '11-integrations-endpoints-open');
});

test('Home in a narrow window: the stats above the feed, the tip card whole', async ({ main, shot }) => {
  const cdp = await main.context().newCDPSession(main);
  try {
    await cdp.send('Emulation.setDeviceMetricsOverride', { width: 860, height: 600, deviceScaleFactor: 1, mobile: false });
    await openView(main, 'Home');
    const stats = await main.locator('.statcard').boundingBox();
    const hero = await main.locator('.hero').boundingBox();
    // Stacked: the numbers in one row above the tip card, which keeps its
    // width (it used to squeeze into a column of one word per line).
    expect(stats.y + stats.height).toBeLessThanOrEqual(hero.y);
    expect(stats.height).toBeLessThan(110);
    expect(hero.width).toBeGreaterThan(450);
    expect(hero.height).toBeLessThan(260);
    expect(await main.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
    await shot(main, '02-home-narrow');
  } finally {
    await cdp.send('Emulation.clearDeviceMetricsOverride').catch(() => {});
    await cdp.detach().catch(() => {});
  }
});

test('the Copilot key as the dictation key, from the shortcut picker', async ({ yap, main, shot }) => {
  const dialog = await openSettings(main, 'Shortcuts');
  const dictate = dialog.locator('[data-setting="shortcut-dictation"]');
  await dictate.getByRole('button', { name: /Change the shortcut|F24/ }).first().click();
  // Windows may open Copilot before Yap's window sees the key: a button.
  const copilot = dictate.getByRole('button', { name: 'Copilot key' });
  await expect(copilot).toBeVisible();
  await shot(main, '03-shortcut-picker-copilot');
  await copilot.click();
  await expectStore(yap, 'config.json', (c) => c.hotkey === COPILOT_KEY);
  await expect(dictate.locator('kbd')).toHaveText(['Copilot']);

  // Push-to-talk says what a Copilot key may not do.
  await openSettings(main, 'Dictation');
  await dialog.getByRole('tab', { name: 'Push-to-talk' }).click();
  await expect(dialog.locator('[data-setting="recording-mode"]')).toContainText('Hold the Copilot key while you talk');
  await shot(main, '04-copilot-push-to-talk');
  await dialog.getByRole('tab', { name: 'Toggle' }).click();
  await closeSettings(main);

  // Home greets with it.
  await openView(main, 'Insights');
  await openView(main, 'Home');
  await expect(main.getByRole('heading', { level: 1 })).toContainText('Press Copilot to start yapping');

  // Back to the suite's F24, through the picker (Settings' own copy of the
  // config would put the Copilot key back on its next auto-save otherwise).
  await openSettings(main, 'Shortcuts');
  await dictate.getByRole('button', { name: /Change$/ }).click();
  await pressHotkey(main);
  await expectStore(yap, 'config.json', (c) => c.hotkey === 'kb:135' && c.recordingMode === 'toggle');
  await closeSettings(main);
});

test('onboarding: the main button in view, the tray step, the shortcut picker, ending on Yap', async ({ yap, main, shot }) => {
  const ob = yap.onboarding;
  await yap.invoke('open_onboarding');
  await expect(ob.getByRole('heading', { level: 1, name: 'Welcome to Yap' })).toBeVisible();

  // Speech models: the recommended one and the best other one for this
  // PC's language (Parakeet V2 for English), the rest behind "More models",
  // and the Download button in view without scrolling.
  const english = (await ob.evaluate(() => navigator.language)).toLowerCase().startsWith('en');
  const cards = ob.locator('.cards .card');
  await expect(cards).toHaveCount(2);
  await expect(cards.locator('h3')).toHaveText(['Parakeet V3', english ? 'Parakeet V2' : 'Whisper Large v3 Turbo']);
  await expect(cards.first().locator('.badge')).toHaveText('Recommended');
  const download = ob.getByRole('button', { name: /^Download Parakeet V3/ });
  await expect(download).toBeInViewport();
  await shot(ob, '12-onboarding-speech-models-short');
  await ob.getByRole('button', { name: 'More models (12)' }).click();
  await expect(cards).toHaveCount(14);
  await expect(cards.locator('h3').first()).toHaveText('Parakeet V3');
  await expect(ob.getByRole('button', { name: /^More models/ })).toHaveCount(0);
  await ob.getByRole('button', { name: "I'll choose later" }).click();
  await ob.getByRole('button', { name: 'Continue →' }).click();

  // AI cleanup: the model for this PC and one more, the rest behind "More
  // models", and the Download button visible without scrolling.
  const status = await yap.invoke('local_llm_status');
  const total = status.curated.length;
  await expect(ob.locator('.llm-row')).toHaveCount(Math.min(2, total));
  await expect(ob.locator('.llm-row').first().locator('.llm-badge')).toHaveText('Recommended for this PC');
  const cta = ob.getByRole('button', { name: /& enable$|^Enable / });
  const box = await ob.locator('.cleanup-box').boundingBox();
  const btn = await cta.boundingBox();
  expect(btn.y + btn.height).toBeLessThanOrEqual(box.y + box.height);
  await shot(ob, '05-onboarding-cleanup-short');
  await ob.getByRole('button', { name: `More models (${total - 2})` }).click();
  await expect(ob.locator('.llm-row')).toHaveCount(total);
  await ob.getByRole('button', { name: 'Maybe later' }).click();

  // The tray step: today's Yap (the Yap bar, open Yap), the footer at the bottom.
  await expect(ob.getByRole('heading', { level: 1, name: 'Yap lives in your tray' })).toBeVisible();
  await expect(ob.locator('header .sub')).toContainText('Yap bar');
  await expect(ob.locator('.tray-tips')).toContainText('Click the icon to open Yap.');
  await expect(ob.locator('.tray-tips')).not.toContainText('Settings');
  const footer = await ob.locator('footer').boundingBox();
  const height = await ob.evaluate(() => window.innerHeight);
  expect(footer.y + footer.height).toBeGreaterThan(height - 40);
  await shot(ob, '06-onboarding-tray');
  await ob.getByRole('button', { name: 'Continue →' }).click();

  // Try it: Settings' own shortcut picker, combos and the Copilot key.
  const picker = ob.getByRole('group', { name: 'Dictation shortcut' });
  await expect(picker.locator('kbd')).toHaveText(['F24']);
  await picker.getByRole('button', { name: /Change the shortcut|F24/ }).first().click();
  await expect(picker.getByRole('button', { name: 'Copilot key' })).toBeVisible();
  await shot(ob, '07-onboarding-shortcut-picker');
  // A combo is kept whole (the old recorder dropped the modifiers).
  await ob.keyboard.press('Control+Shift+KeyY');
  await expectStore(yap, 'config.json', (c) => c.hotkey === 'kb:ctrl+shift+89');
  await expect(picker.locator('kbd')).toHaveText(['Ctrl', 'Shift', 'Y']);
  await expect(ob.locator('header .sub')).toContainText('Ctrl + Shift + Y');

  // Finish: Yap's window comes up (it was hidden: Yap lives in the tray).
  await yap.invoke('plugin:window|close', { label: 'settings' }); // close = hide
  await expect.poll(() => yap.invoke('plugin:window|is_visible', { label: 'settings' })).toBe(false);
  await ob.getByRole('button', { name: /^Finish/ }).click();
  await expect.poll(() => yap.invoke('plugin:window|is_visible', { label: 'onboarding' })).toBe(false);
  await expect.poll(() => yap.invoke('plugin:window|is_visible', { label: 'settings' })).toBe(true);

  const cfg = await yap.invoke('get_config');
  await yap.invoke('save_config', { cfg: { ...cfg, hotkey: 'kb:135' } });
  await yap.invoke('configure_hotkey', { spec: 'kb:135' });
  await mainOnScreen(yap);
});

test("Chat says what to do when its AI model can't be reached", async ({ main, deadPort, shot }) => {
  await openView(main, 'Chat');
  const box = main.getByRole('textbox', { name: 'Message' });
  await box.fill('Hello?');
  await box.press('Enter');
  const toast = main.getByRole('status').filter({ hasText: "Couldn't send" });
  await expect(toast).toContainText(`Couldn't reach the AI model on this PC (127.0.0.1:${deadPort}). Check that it's running`);
  // A sentence, not the raw error in a box; the message is given back.
  await expect(toast.locator('.errbox')).toHaveCount(0);
  await expect(box).toHaveValue('Hello?');
  await shot(main, '08-chat-unreachable');
  await toast.getByRole('button', { name: 'Open Language Models' }).click();
  await expect(settingsDialog(main).getByRole('heading', { level: 1, name: 'Language Models' })).toBeVisible();
  await closeSettings(main);
});

test('an empty Chat with no AI model says what it needs', async ({ yap, main, shot }) => {
  const before = await yap.invoke('get_config');
  try {
    await yap.invoke('save_config', { cfg: { ...before, llmScopes: {}, postProcessEnabled: false } });
    await openView(main, 'Chat');
    await main.getByRole('button', { name: 'New chat' }).click();
    const needs = main.locator('.needsai');
    await expect(needs).toContainText('Chat needs an AI model');
    await shot(main, '10-chat-needs-a-model');
    await needs.getByRole('button', { name: 'Set up an AI model' }).click();
    await expect(settingsDialog(main).getByRole('heading', { level: 1, name: 'Language Models' })).toBeVisible();
    await closeSettings(main);
  } finally {
    await yap.invoke('save_config', { cfg: before });
  }
  await expect(main.locator('.needsai')).toHaveCount(0);
});

test("what needs doing is counted once, on the title bar's bell", async ({ main, shot }) => {
  // The stub build has no speech model: one item ("No speech-to-text model
  // installed"). The bell counts and names it; the Settings row doesn't
  // repeat the count (it showed "1" beside the bell's "1").
  const bell = main.getByRole('button', { name: /^Notifications/ });
  const count = await bell.locator('.belldot').textContent();
  expect(Number(count)).toBeGreaterThanOrEqual(1);
  await expect(bell).toHaveAccessibleName(`Notifications (${count})`);
  const settingsRow = main.getByRole('navigation', { name: 'Main' }).getByRole('button', { name: 'Settings', exact: true });
  await expect(settingsRow).toBeVisible();
  await expect(settingsRow).toHaveText('Settings');
  await bell.click();
  await expect(bell).toHaveAttribute('aria-expanded', 'true');
  const items = main.locator('.bellmenu .bellitem');
  await expect(items).toHaveCount(Number(count));
  await expect(items.filter({ hasText: 'No speech-to-text model installed' })).toHaveCount(1);
  await shot(main, '13-bell-the-one-count');
  await items.filter({ hasText: 'No speech-to-text model installed' }).click();
  await expect(settingsDialog(main).getByRole('heading', { level: 1, name: 'Speech-to-Text' })).toBeVisible();
  await closeSettings(main);
});

test('Home with no speech model yet says so first, and leads to the download', async ({ yap, main, shot }) => {
  // Last: it clears the seeded history, and the stats (they outlast
  // history: dictation/stats.rs), so it's someone who hasn't dictated yet.
  // (The stub build has no model.)
  await yap.invoke('clear_history');
  await yap.invoke('e2e_stats_set', {});
  await openView(main, 'Insights');
  await openView(main, 'Home');
  await expect(main.getByRole('heading', { level: 2, name: 'One step left: a speech model' })).toBeVisible();
  // Nothing dictated yet: the tip card starts on the basics, AI cleanup.
  await expect(main.locator('.hero h2')).toHaveText('Speak rough, send polished');
  await shot(main, '09-home-no-model');
  await main.getByRole('button', { name: 'Download a speech model' }).click();
  await expect(settingsDialog(main).getByRole('heading', { level: 1, name: 'Speech-to-Text' })).toBeVisible();
  await closeSettings(main);
});
