// Paste privacy & smart insert (text_injector.rs, selection.rs,
// smart_insert.rs): "Paste last transcript" and Settings → Dictation →
// Typing into apps' "Match the text around the cursor". A test run never pastes,
// copies or reads another app's text (e2e.rs), so the paste itself, the
// clipboard's privacy formats and the text around the cursor are unit-tested
// in Rust (`smart_insert`, and the `--ignored` real-window/clipboard tests);
// here: what Yap would paste, and the setting.
import fs from 'node:fs';
import { test, expect, openSettings, expectStore } from './support/fixtures.js';

const now = Math.floor(Date.now() / 1000);
const FIRST = { ts: now - 300, raw: 'first note', text: 'First note.', model: 'parakeet-tdt-0.6b-v3', app: 'notepad.exe' };
const LATEST = {
  ts: now - 120,
  raw: 'dear sam the report is attached',
  text: 'Dear Sam, the report is attached.',
  model: 'parakeet-tdt-0.6b-v3',
  app: 'OUTLOOK.EXE',
};
// An Upload after them: a transcript, but not a dictation.
const UPLOAD = { ts: now - 60, raw: 'a talk', text: 'A whole talk, transcribed.', model: 'parakeet-tdt-0.6b-v3', app: 'talk.mp3' };

test.use({ yapOptions: { name: 'paste', data: { 'history.json': [FIRST, LATEST, UPLOAD] } } });

const logCount = (yap, text) => fs.readFileSync(yap.logPath, 'utf8').split(text).length - 1;

test('paste last transcript: the latest dictation, nothing pasted in a test run', async ({ yap }) => {
  const before = logCount(yap, 'e2e: not pasting the last transcript');
  // After a restart Yap has only history to go on: the newest dictation,
  // not the newer Upload.
  expect(await yap.invoke('paste_last_transcript')).toBe(LATEST.text);
  expect(logCount(yap, 'e2e: not pasting the last transcript')).toBe(before + 1);

  // Deleted from the Home feed: the one before it.
  await yap.invoke('delete_history_entry', { ts: LATEST.ts, text: LATEST.text });
  expect(await yap.invoke('paste_last_transcript')).toBe(FIRST.text);

  // History cleared: nothing to paste, and it says so.
  await yap.invoke('clear_history');
  const error = await yap.main.evaluate(() =>
    window.__TAURI_INTERNALS__.invoke('paste_last_transcript').then(
      () => null,
      (e) => String(e)
    )
  );
  expect(error).toMatch(/Nothing to paste yet/);
});

test('settings: "Match the text around the cursor" is on, and off is saved', async ({ yap, main, shot }) => {
  const dialog = await openSettings(main, 'Dictation');
  const output = dialog.getByRole('group', { name: 'Typing into apps' });
  const smart = output.getByRole('button', { name: 'Match the text around the cursor' });
  await expect(smart).toHaveAttribute('aria-pressed', 'true');
  // With it on, the trailing space is only for fields Yap can't read.
  await expect(output.getByText("Only where Yap can't read the text around the cursor")).toBeVisible();
  await expect(output.getByText(/Dictations never show up in Win\+V history/)).toBeVisible();
  await output.scrollIntoViewIfNeeded();
  await shot(main, '01-settings-output');

  await smart.click();
  await expect(smart).toHaveAttribute('aria-pressed', 'false');
  await expectStore(yap, 'config.json', (c) => c.smartSpacing === false);
  await expect(output.getByText('A space after each dictation')).toBeVisible();
  await shot(main, '02-settings-output-off');

  await smart.click();
  await expectStore(yap, 'config.json', (c) => c.smartSpacing === true);
});

// What they do to a dictation (app_kind.rs: telling terminals and chat apps
// apart, one line at a shell, a chat message's full stop, which key follows
// the paste) is unit-tested in Rust: a test run reads no other app's window
// and pastes nothing.
test('settings: chat full stops and safer terminal pasting are on, and off is saved', async ({ yap, main, shot }) => {
  const dialog = await openSettings(main, 'Dictation');
  const output = dialog.getByRole('group', { name: 'Typing into apps' });
  const chat = output.getByRole('button', { name: 'Leave the full stop off chat messages' });
  const terminal = output.getByRole('button', { name: 'Safer pasting into terminals' });
  await expect(chat).toHaveAttribute('aria-pressed', 'true');
  await expect(terminal).toHaveAttribute('aria-pressed', 'true');
  await expect(output.getByText(/a short message ends without one/)).toBeVisible();
  await expect(output.getByText(/One line, and Enter only when you say “press enter”/)).toBeVisible();
  // Auto-submit says where it doesn't press Enter.
  await expect(output.getByText(/In a terminal, only for coding tools like Claude Code/)).toBeVisible();
  await terminal.scrollIntoViewIfNeeded();
  await shot(main, '03-settings-chat-and-terminals');

  await chat.click();
  await expect(chat).toHaveAttribute('aria-pressed', 'false');
  await expectStore(yap, 'config.json', (c) => c.chatNoFullStop === false && c.terminalMode === true);
  await terminal.click();
  await expect(terminal).toHaveAttribute('aria-pressed', 'false');
  await expectStore(yap, 'config.json', (c) => c.terminalMode === false);
  await expect(output.getByText('Sends the message for you', { exact: true })).toBeVisible();
  await shot(main, '04-settings-chat-and-terminals-off');

  await chat.click();
  await terminal.click();
  await expectStore(yap, 'config.json', (c) => c.chatNoFullStop === true && c.terminalMode === true);
});
