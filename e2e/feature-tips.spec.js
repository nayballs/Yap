// Feature tips (2026-10-29; dictation/tips.rs, notices/featureTips.js): the
// first time a dictation shows off something Yap did by itself, one short
// tip says so, once each — AI cleanup catching a self-correction, laying out
// a spoken list, framing an email; a snippet expanding; a word fixed from
// the dictionary. Through the notice policy: a Tip toast in the main window
// after the dictation, a Yap bar card when the person's in another app,
// none with "Show tips" off or with Feature tips off (Settings → General →
// Notifications), and at most three a day once the caps count
// (`e2e_notice_caps`). A test run notices them only once a spec asks
// (`e2e_feature_tips`, which also forgets those shown). The fake AI stands
// in for cleanup; dictations go to stand-in apps (`e2e_style_app`).
import { test as base, expect, closeToasts, expectStore, inAnotherApp, setConfig } from './support/fixtures.js';
import { startFakeLlm, cleanupTranscript } from './support/fake-llm.js';

const test = base.extend({
  fakeLlm: [
    async ({}, use) => {
      const fake = await startFakeLlm();
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({ fakeLlm }, use) => {
      await use({
        name: 'feature-tips',
        env: { YAP_E2E_GPU: 'none' },
        config: {
          postProcessEnabled: true,
          ppProvider: 'custom',
          ppBaseUrl: fakeLlm.base,
          ppModel: 'fake-cleanup',
          ppApiKey: '',
          dictionary: [
            { from: 'jaison', to: 'JSON', fuzzy: false, snippet: false },
            { from: 'my sign off', to: 'Speak soon,\nRobin', fuzzy: true, snippet: true },
          ],
        },
      });
    },
    { scope: 'worker' },
  ],
});

const tip = (main, title) => main.getByRole('status').filter({ hasText: title });

async function dictate(yap, exe, title, text) {
  await yap.invoke('e2e_style_app', { exe, title });
  return yap.invoke('e2e_dictation', { text });
}

test.beforeEach(async ({ yap }) => {
  await yap.invoke('e2e_feature_tips', { on: true });
});

test.afterEach(async ({ yap, fakeLlm }) => {
  fakeLlm.reply('cleanup', cleanupTranscript);
  await yap.invoke('e2e_feature_tips', { on: false }).catch(() => {});
  await yap.invoke('e2e_style_app', { exe: null, title: null }).catch(() => {});
});

test('a self-correction caught: its tip once, and not for the same thing again', async ({ yap, main, fakeLlm, shot }) => {
  fakeLlm.reply('cleanup', () => "Let's meet on Thursday at ten.");
  await dictate(yap, 'notepad.exe', 'notes.txt - Notepad', "let's meet on Wednesday no wait Thursday at ten");
  const t = tip(main, 'Yap caught your correction');
  await expect(t).toBeVisible();
  await expect(t).toContainText('kept only what you meant');
  await shot(main, '01-self-correction');
  await expectStore(yap, 'hints.json', (h) => !!h.features?.selfCorrection);
  await closeToasts(main);
  // Once: the same again says nothing.
  await dictate(yap, 'notepad.exe', 'notes.txt - Notepad', "send it to Priya sorry to Sam today");
  await main.waitForTimeout(2_500);
  await expect(tip(main, 'Yap caught your correction')).toHaveCount(0);
});

test('a spoken list laid out, and an email framed', async ({ yap, main, fakeLlm, shot }) => {
  fakeLlm.reply('cleanup', () => 'For the offsite we need:\n1. Book the venue\n2. Sort out the catering\n3. Send the invites');
  await dictate(yap, 'notepad.exe', 'notes.txt - Notepad', 'for the offsite we need first book the venue second sort out the catering third send the invites');
  await expect(tip(main, 'Yap made that a list')).toBeVisible();
  await shot(main, '02-list');
  await closeToasts(main);

  // The next kind waits ten minutes after one: forgotten here, as a day later.
  await yap.invoke('e2e_feature_tips', { on: true });
  fakeLlm.reply('cleanup', () => 'Hi Sam,\n\nThanks for the figures.\n\nCheers,');
  await dictate(yap, 'OUTLOOK.EXE', 'Inbox - Outlook', 'hi sam thanks for the figures cheers');
  await expect(tip(main, 'Yap laid that out as an email')).toBeVisible();
  await shot(main, '03-email');
  await closeToasts(main);
});

test('a snippet expanded and a dictionary word fixed, each with Open Dictionary', async ({ yap, main, shot }) => {
  await dictate(yap, 'notepad.exe', 'notes.txt - Notepad', 'thanks again my sign off');
  const snippet = tip(main, 'Your snippet expanded');
  await expect(snippet).toBeVisible();
  await expect(snippet.getByRole('button', { name: 'Open Dictionary' })).toBeVisible();
  await shot(main, '04-snippet');
  await closeToasts(main);

  await yap.invoke('e2e_feature_tips', { on: true });
  await dictate(yap, 'notepad.exe', 'notes.txt - Notepad', 'parse the jaison file');
  const word = tip(main, 'Fixed from your dictionary');
  await expect(word).toBeVisible();
  await word.getByRole('button', { name: 'Open Dictionary' }).click();
  await expect(main.getByRole('heading', { level: 1, name: 'Dictionary' })).toBeVisible();
});

test('in another app the tip is a Yap bar card; Show tips or Feature tips off, none', async ({ yap, main, shot }) => {
  await inAnotherApp(yap);
  await dictate(yap, 'notepad.exe', 'notes.txt - Notepad', 'parse the jaison file');
  const card = yap.overlay.getByRole('status').filter({ hasText: 'Fixed from your dictionary' });
  await expect(card).toBeVisible();
  await shot(yap.overlay, '05-bar-card');
  await yap.invoke('e2e_focus', { focused: true });
  await expect(card).toBeHidden({ timeout: 20_000 });

  for (const off of [{ showTips: false }, { notifyFeatureTips: false }]) {
    await setConfig(yap, main, off);
    try {
      await yap.invoke('e2e_feature_tips', { on: true });
      await dictate(yap, 'notepad.exe', 'notes.txt - Notepad', 'thanks again my sign off');
      await main.waitForTimeout(2_500);
      await expect(tip(main, 'Your snippet expanded')).toHaveCount(0);
      expect(yap.readJson('hints.json').features?.snippet).toBeUndefined();
    } finally {
      await setConfig(yap, main, { showTips: true, notifyFeatureTips: true });
    }
  }
});

test('at most three feature tips a day, once the caps count', async ({ yap, main }) => {
  await yap.invoke('e2e_notice_caps', { on: true });
  try {
    for (const text of ['parse the jaison file', 'parse the jaison file', 'parse the jaison file']) {
      await yap.invoke('e2e_feature_tips', { on: true });
      await dictate(yap, 'notepad.exe', 'notes.txt - Notepad', text);
      await expect(tip(main, 'Fixed from your dictionary')).toBeVisible();
      await closeToasts(main);
    }
    // The fourth today: not shown, not counted as shown.
    await yap.invoke('e2e_feature_tips', { on: true });
    await dictate(yap, 'notepad.exe', 'notes.txt - Notepad', 'parse the jaison file');
    await main.waitForTimeout(2_500);
    await expect(tip(main, 'Fixed from your dictionary')).toHaveCount(0);
    expect(yap.readJson('hints.json').features?.dictionary).toBeUndefined();
  } finally {
    await yap.invoke('e2e_notice_caps', { on: false });
  }
});
