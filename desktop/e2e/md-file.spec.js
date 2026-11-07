// Markdown files open in Yap (desktop/src-tauri/src/notes/md_file.rs,
// desktop/src/lib/doc/DocWindow.svelte, 2026-11-07): a .md file in a window
// of its own — Preview (a page you can format) or Source (its Markdown,
// coloured) —, saved back to the file as you type with its byte-order mark
// and line endings kept, following changes on disk (reloaded in place with
// nothing unsaved; "Changed on disk · Reload · Keep mine" with unsaved
// edits). One window per file. Settings → General → Markdown files.
//
// The files are temp copies in this run's own folder (fictional text). A
// test run opens nothing outside Yap: "Open folder" and "Open Default apps"
// are kept for the suite (`e2e_opened_urls`). "Explorer → Open with" and
// `yap.exe <path>` are the installer's and a launch's (unit-tested in Rust:
// `notes::md_file::args`); here a file comes in through `open_markdown`, the
// command Settings' "Open a Markdown file…" calls after its dialog.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openSettings, closeSettings, setConfig } from './support/fixtures.js';
import { pageIn } from './support/notePage.js';
import { afterRead, statusLine, formLine } from '../src/lib/doc/docFile.js';

const test = base.extend({
  yapOptions: [{ name: 'md-file' }, { scope: 'worker', option: true }],
});

const BOM = '﻿';
// A plan, as a person or an assistant writes one (fictional), long enough to
// scroll; written with a byte-order mark and Windows' line endings.
const LINES = [
  '# Garden plan',
  '',
  'A fictional plan for the spring, kept in a Markdown file.',
  '',
  '## Beds',
  '',
  '- Sow the beans',
  '- Mend the fence',
  '',
  '| Bed | Crop |',
  '|---|---|',
  '| North | Peas |',
  '',
  '## Diary',
  '',
  ...Array.from({ length: 60 }, (_, i) => `Day ${i + 1}: watered the beds and noted the weather.`),
  '',
];
const PLAN = LINES.join('\n');
const onDisk = (text) => BOM + text.replace(/\n/g, '\r\n');

/** A fresh copy of `text` (page form, `\n`) in this run's folder, as the file writes it. */
function fixture(yap, name, text = PLAN) {
  const dir = path.join(yap.runDir, 'md-files');
  fs.mkdirSync(dir, { recursive: true });
  const file = path.join(dir, name);
  fs.writeFileSync(file, onDisk(text), 'utf8');
  return file;
}

const read = (file) => fs.readFileSync(file, 'utf8');

/** A webview's Tauri window label. */
const labelOf = (page) => page.evaluate(() => window.__TAURI_INTERNALS__?.metadata?.currentWindow?.label ?? null).catch(() => null);

/** The labels of the Markdown files' windows open now. */
async function docLabels(yap) {
  const out = [];
  for (const page of yap.browser.contexts()[0].pages()) {
    const l = await labelOf(page);
    if (l?.startsWith('doc-')) out.push(l);
  }
  return out;
}

/** The page of the window labelled `label` (made on demand), its errors watched. */
async function docPage(yap, label) {
  const context = yap.browser.contexts()[0];
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    for (const page of context.pages()) {
      if ((await labelOf(page)) === label) {
        if (!page.__watched) {
          page.__watched = true;
          page.on('pageerror', (err) => yap.pageErrors.push({ label, message: err.stack || String(err) }));
        }
        return page;
      }
    }
    await new Promise((r) => setTimeout(r, 200));
  }
  throw new Error(`The window "${label}" never showed up`);
}

const pagesFor = async (yap, label) => (await docLabels(yap)).filter((l) => l === label).length;

const source = (doc) => doc.getByRole('textbox', { name: 'Markdown source' });
const status = (doc) => doc.getByTestId('doc-status');
const bar = (doc) => doc.getByTestId('doc-bar');

async function openFile(yap, file) {
  const label = await yap.invoke('open_markdown', { path: file });
  expect(label).toMatch(/^doc-[0-9a-f]{12}$/);
  const doc = await docPage(yap, label);
  await expect(doc.getByRole('heading', { level: 1, name: path.basename(file) })).toBeVisible({ timeout: 15_000 });
  return { label, doc };
}

async function toView(doc, name) {
  await doc.getByRole('group', { name: 'View' }).getByRole('button', { name }).click();
  await expect(doc.getByRole('group', { name: 'View' }).getByRole('button', { name })).toHaveAttribute('aria-pressed', 'true');
}

/** The caret at the end of `line` in Source's box. */
async function caretAfter(box, line) {
  await box.evaluate((el, l) => {
    el.focus();
    const at = el.value.indexOf(l) + l.length;
    el.setSelectionRange(at, at);
  }, line);
}

// ---------------------------------------------------------------- no app

test('what a read of the file does to the page', () => {
  // In step: the file holds the page's words.
  expect(afterRead({ text: 'a', base: 'old', dirty: true }, 'a')).toBe('same');
  // Nothing unsaved: the file's words in place.
  expect(afterRead({ text: 'a', base: 'a', dirty: false }, 'b')).toBe('reload');
  // Only the file's time changed: the edits stay.
  expect(afterRead({ text: 'mine', base: 'a', dirty: true }, 'a')).toBe('touched');
  // Changed there and here.
  expect(afterRead({ text: 'mine', base: 'a', dirty: true }, 'theirs')).toBe('conflict');
  expect(statusLine('saved')).toBe('Saved');
  expect(statusLine('')).toBe('');
  expect(formLine({ encoding: 'UTF-8 with BOM', eol: 'CRLF' })).toBe('UTF-8 with BOM · CRLF');
});

// ---------------------------------------------------------------- the window

test('a Markdown file opens in its window: Preview renders it, Source edits it, saved with its BOM and line endings', async ({ yap, main, shot }) => {
  const file = fixture(yap, 'garden-plan.md');
  const { label, doc } = await openFile(yap, file);
  await expect(doc.getByText(path.dirname(file), { exact: true })).toBeVisible();

  // Preview: the page, as Notes draws one.
  await toView(doc, 'Preview');
  const page = doc.getByRole('textbox', { name: 'garden-plan.md' });
  await pageIn(page);
  await expect(page.getByRole('heading', { name: 'Garden plan' })).toBeVisible();
  await expect(page.getByRole('heading', { name: 'Beds' })).toBeVisible();
  await expect(page.getByRole('listitem').filter({ hasText: 'Sow the beans' })).toBeVisible();
  await expect(page.getByRole('cell', { name: 'Peas' })).toBeVisible();
  await expect(doc.getByText('UTF-8 with BOM · CRLF')).toBeVisible();
  await shot(doc, '01-preview');

  // Opening it again brings the same window up: one per file.
  expect(await yap.invoke('open_markdown', { path: file })).toBe(label);
  expect(await pagesFor(yap, label)).toBe(1);

  // Source: the Markdown, coloured and editable; saved to the file.
  await toView(doc, 'Source');
  const box = source(doc);
  await expect(box).toHaveValue(PLAN);
  await expect(doc.locator('.srcmirror .md-s-heading').first()).toHaveText('# Garden plan');
  await caretAfter(box, '- Mend the fence');
  await doc.keyboard.press('Enter');
  await doc.keyboard.insertText('- Water the roses');
  const edited = PLAN.replace('- Mend the fence\n', '- Mend the fence\n- Water the roses\n');
  await expect.poll(() => read(file), { timeout: 10_000 }).toBe(onDisk(edited));
  await expect(status(doc)).toHaveText('Saved');
  // No temp file left beside it.
  expect(fs.readdirSync(path.dirname(file)).filter((n) => n.startsWith('garden-plan'))).toEqual(['garden-plan.md']);
  await shot(doc, '02-source-saved');

  // An edit on the page in Preview: the file keeps its Markdown, only that line new.
  await toView(doc, 'Preview');
  await pageIn(page);
  await page.getByText('Sow the beans').click();
  await doc.keyboard.press('End');
  await doc.waitForTimeout(80);
  await doc.keyboard.insertText(' early');
  await expect.poll(() => read(file), { timeout: 10_000 }).toBe(onDisk(edited.replace('- Sow the beans', '- Sow the beans early')));

  // Find (Ctrl+F): on the page, and in Source.
  await doc.keyboard.press('Control+f');
  await expect(doc.getByRole('textbox', { name: 'Find in the note' })).toBeFocused();
  await doc.keyboard.press('Escape');
  await toView(doc, 'Source');
  await source(doc).focus();
  await doc.keyboard.press('Control+f');
  const findBox = doc.getByRole('textbox', { name: 'Find in the file' });
  await expect(findBox).toBeFocused();
  // "## Beds" and each day's "watered the beds".
  await doc.keyboard.insertText('beds');
  await expect(doc.getByRole('search', { name: 'Find in the file' })).toContainText('1 of 61');
  await doc.keyboard.press('Escape');
});

test('a change on disk reloads in place, keeping the scroll; with unsaved edits it asks: Reload or Keep mine', async ({ yap, main, shot }) => {
  const file = fixture(yap, 'diary.md');
  const { doc } = await openFile(yap, file);
  await toView(doc, 'Source');
  const box = source(doc);
  await expect(box).toHaveValue(PLAN);

  // Nothing unsaved: another app's change comes in, where it was scrolled to.
  const scroller = doc.locator('.srcscroll');
  await scroller.evaluate((el) => (el.scrollTop = 600));
  const theirs = PLAN.replace('Day 60: watered the beds and noted the weather.', 'Day 60: a frost; covered the peas.');
  fs.writeFileSync(file, onDisk(theirs), 'utf8');
  await expect(box).toHaveValue(theirs, { timeout: 10_000 });
  await expect(bar(doc)).toHaveCount(0);
  expect(Math.abs((await scroller.evaluate((el) => el.scrollTop)) - 600)).toBeLessThan(4);
  // Nothing was written back: the file is theirs, byte for byte.
  expect(read(file)).toBe(onDisk(theirs));

  // And in Preview: the page follows too.
  await toView(doc, 'Preview');
  await pageIn(doc.getByRole('textbox', { name: 'diary.md' }));
  const theirs2 = theirs.replace('## Diary', '## Garden diary');
  fs.writeFileSync(file, onDisk(theirs2), 'utf8');
  await expect(doc.getByRole('heading', { name: 'Garden diary' })).toBeVisible({ timeout: 10_000 });
  await toView(doc, 'Source');
  await expect(box).toHaveValue(theirs2);

  // Typed here and not saved yet, changed there: the bar, nothing written.
  await caretAfter(box, 'Day 1: watered the beds and noted the weather.');
  await doc.keyboard.insertText(' Mine.');
  const other = theirs2.replace('Day 2: watered', 'Day 2: weeded');
  fs.writeFileSync(file, onDisk(other), 'utf8');
  await expect(bar(doc)).toContainText('Changed on disk', { timeout: 10_000 });
  await expect(bar(doc).getByRole('button', { name: 'Reload' })).toBeVisible();
  await expect(bar(doc).getByRole('button', { name: 'Keep mine' })).toBeVisible();
  await doc.waitForTimeout(1_200);
  expect(read(file)).toBe(onDisk(other));
  await shot(doc, '03-changed-on-disk');

  // Keep mine: what's here written over it, as the file was written.
  await bar(doc).getByRole('button', { name: 'Keep mine' }).click();
  const mine = theirs2.replace('Day 1: watered the beds and noted the weather.', 'Day 1: watered the beds and noted the weather. Mine.');
  await expect.poll(() => read(file), { timeout: 10_000 }).toBe(onDisk(mine));
  await expect(bar(doc)).toHaveCount(0);

  // Again, and this time Reload: the file's words, mine let go.
  await caretAfter(box, 'Day 3: watered the beds and noted the weather.');
  await doc.keyboard.insertText(' Scrap this.');
  const third = mine.replace('Day 4: watered', 'Day 4: mulched');
  fs.writeFileSync(file, onDisk(third), 'utf8');
  await expect(bar(doc)).toContainText('Changed on disk', { timeout: 10_000 });
  await bar(doc).getByRole('button', { name: 'Reload' }).click();
  await expect(bar(doc)).toHaveCount(0);
  await expect(box).toHaveValue(third);
  await doc.waitForTimeout(1_500);
  expect(read(file)).toBe(onDisk(third));

  // Moved away: said so; "Save it here again" puts it back.
  fs.renameSync(file, `${file}.moved`);
  await expect(bar(doc)).toContainText('Moved or deleted on disk', { timeout: 10_000 });
  await bar(doc).getByRole('button', { name: 'Save it here again' }).click();
  await expect.poll(() => fs.existsSync(file) && read(file), { timeout: 10_000 }).toBe(onDisk(third));
  fs.rmSync(`${file}.moved`);
});

test('too big to open, said plainly; the title bar opens the folder and closes the window; dark theme', async ({ yap, main, shot }) => {
  // Over 5 MB.
  const big = fixture(yap, 'huge-log.md', '# Log\n');
  fs.writeFileSync(big, `# Log\n${'x'.repeat(5 * 1024 * 1024)}\n`);
  const huge = await openFile(yap, big);
  await expect(huge.doc.getByRole('alert').filter({ hasText: '5 MB' })).toContainText('This file is just over 5 MB: Yap opens Markdown files up to 5 MB.');
  await shot(huge.doc, '04-too-big');
  await huge.doc.getByRole('button', { name: 'Close', exact: true }).click();
  await expect.poll(() => pagesFor(yap, huge.label)).toBe(0);

  const file = fixture(yap, 'notes.markdown', '# Notes\n\nShort and sweet.\n');
  const { label, doc } = await openFile(yap, file);
  // (Each window opens on the view the last one was left on.)
  await toView(doc, 'Preview');
  await pageIn(doc.getByRole('textbox', { name: 'notes.markdown' }));

  // Open folder: Explorer with the file selected (kept, in a test run).
  await doc.getByRole('button', { name: 'Open folder', exact: true }).click();
  await expect.poll(() => yap.invoke('e2e_opened_urls')).toContain(`reveal:${label}`);
  // Read aloud and Copy are there.
  await expect(doc.getByRole('button', { name: 'Read aloud', exact: true })).toBeVisible();
  await expect(doc.getByRole('button', { name: 'Copy', exact: true })).toBeVisible();

  // Dark: the page and the webview under it (theme.rs) on the dark paper.
  await setConfig(yap, main, { theme: 'dark' });
  await expect(doc.locator('html')).toHaveAttribute('data-yap-theme', 'dark');
  await expect(doc.locator('html')).toHaveAttribute('data-window', 'doc');
  await expect.poll(() => yap.invoke('e2e_webview_background', { label })).toBe('#1c1a16');
  await shot(doc, '05-dark');
  await setConfig(yap, main, { theme: 'light' });
  await expect.poll(() => yap.invoke('e2e_webview_background', { label })).toBe('#faf9f6');
  await setConfig(yap, main, { theme: 'system' });

  // An edit, then ✕: saved on the way out, the window gone.
  await toView(doc, 'Source');
  await caretAfter(source(doc), 'Short and sweet.');
  await doc.keyboard.insertText(' Done.');
  await doc.getByRole('button', { name: 'Close', exact: true }).click();
  await expect.poll(() => pagesFor(yap, label)).toBe(0);
  expect(read(file)).toBe(onDisk('# Notes\n\nShort and sweet. Done.\n'));
  // Opened again: a new window on the file as it is now.
  const again = await openFile(yap, file);
  expect(again.label).toBe(label);
  await toView(again.doc, 'Source');
  await expect(source(again.doc)).toHaveValue('# Notes\n\nShort and sweet. Done.\n');
  await again.doc.getByRole('button', { name: 'Close', exact: true }).click();
  await expect.poll(() => pagesFor(yap, label)).toBe(0);
});

test('Settings → General → Markdown files: Default apps, and a file to open', async ({ yap, main, shot }) => {
  const dialog = await openSettings(main, 'General');
  const group = dialog.getByRole('group', { name: 'Markdown files' });
  const withYap = group.locator('[data-setting="open-markdown-with"]');
  await withYap.scrollIntoViewIfNeeded();
  await expect(withYap).toContainText('Open Markdown files with Yap');
  // What it says follows what Windows knows (a test build isn't installed;
  // a Yap installed on this PC may be registered, even the default).
  const known = await yap.invoke('md_open_with_state');
  await expect(withYap).toContainText(known.isDefault ? 'Yap opens .md' : known.registered ? 'Windows asks you to pick Yap once' : 'isn’t installed');
  await expect(group.locator('[data-setting="open-markdown-file"]')).toContainText('Open a Markdown file…');
  await shot(main, '06-settings');
  await withYap.getByRole('button', { name: 'Open Default apps' }).click();
  await expect.poll(async () => (await yap.invoke('e2e_opened_urls')).some((u) => /^ms-settings:defaultapps/.test(u))).toBe(true);
  await closeSettings(main);
});
