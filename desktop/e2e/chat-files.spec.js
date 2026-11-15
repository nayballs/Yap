// Files attached to a question (desktop/src-tauri/src/chat/attachments.rs, 15 Nov 2026; the Claude app's + and drop):
// - A text file and a PDF dropped on Chat wait in the box as chips — a type badge, the name and the size, as the
//   Claude app's — beside a picture dropped with them (that one a screenshot tile). A file Chat can't read is said
//   plainly, never added.
// - Sent: their text goes before the question (a <documents> block; the PDF's read on this PC), the question keeps
//   their chips (chats.json `attachments`), a follow-up sends them again in the history, a chip opens its file.
// - ✕ takes one out and deletes it; one too big for the model is said before it's sent; deleting the chat deletes
//   its files; Settings → Privacy & data → Chat files counts them and deletes them all.
//
// The model is a local fake (support/fake-llm.js). Fictional files only.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { test as base, expect, expectStore, openView, openSettings, closeSettings } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { writePng } from './support/png.js';
import { badgeOf, fileSize, filesTokens, isPicturePath, fileTitle, filesPrompt, skippedLine } from '../src/lib/views/chat/chatFiles.js';

const HERE = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', '..', 'test-results', 'chat-files');
const SEEDS = path.join(HERE, 'seed-list.txt');
const ORDER = path.join(HERE, 'seed-order.pdf');
const BEDS = path.join(HERE, 'beds.png');
const BUDGET = path.join(HERE, 'budget.xlsx');
const HUGE = path.join(HERE, 'allotment-log.txt');

/** A one-page PDF with `text` on it, its cross-reference table right (made here: nobody's file). */
function tinyPdf(text) {
  const content = `BT /F1 12 Tf 72 720 Td (${text.replace(/[\\()]/g, (c) => '\\' + c)}) Tj ET`;
  const objects = [
    '<< /Type /Catalog /Pages 2 0 R >>',
    '<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
    '<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>',
    `<< /Length ${content.length} >>\nstream\n${content}\nendstream`,
    '<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>',
  ];
  let out = '%PDF-1.4\n';
  const offsets = [];
  objects.forEach((o, i) => {
    offsets.push(out.length);
    out += `${i + 1} 0 obj\n${o}\nendobj\n`;
  });
  const xref = out.length;
  out += `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n`;
  for (const off of offsets) out += `${String(off).padStart(10, '0')} 00000 n \n`;
  out += `trailer\n<< /Size ${objects.length + 1} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
  return Buffer.from(out, 'latin1');
}

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
      fs.rmSync(HERE, { recursive: true, force: true });
      fs.mkdirSync(HERE, { recursive: true });
      fs.writeFileSync(SEEDS, 'Runner beans\r\nSweet peas\r\nCourgettes\r\n');
      fs.writeFileSync(ORDER, tinyPdf('Twelve packets of runner beans, due Friday'));
      fs.writeFileSync(BEDS, writePng(96, 60, [90, 140, 70]));
      fs.writeFileSync(BUDGET, Buffer.from([0x50, 0x4b, 0x03, 0x04, 0, 0, 0, 0, 0, 1, 2, 3]));
      await use({
        name: 'chat-files',
        config: {
          webSearch: false,
          meetingReminder: 'never',
          llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat' } },
        },
      });
    },
    { scope: 'worker' },
  ],
});

const box = (main) => main.getByRole('textbox', { name: 'Message' });
const thread = (main) => main.locator('.msgs');
const boxFiles = (main) => main.getByTestId('chat-files').getByTestId('chat-file-chip');
const drop = (yap, paths) => yap.invoke('e2e_drag_drop', { paths });
/** The text of the last user message the model got. */
const lastAsked = (fakeLlm) => {
  const msgs = fakeLlm.requests.filter((r) => r.kind === 'other').at(-1)?.messages || [];
  const m = msgs.filter((x) => x.role === 'user').at(-1);
  return typeof m?.content === 'string' ? m.content : (m?.content || []).filter((p) => p.type === 'text').map((p) => p.text).join('\n');
};
/** Every user message the model got last, as text. */
const allAsked = (fakeLlm) => (fakeLlm.requests.filter((r) => r.kind === 'other').at(-1)?.messages || []).filter((x) => x.role === 'user').map((m) => (typeof m.content === 'string' ? m.content : JSON.stringify(m.content)));

async function freshChat(main) {
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
}

// ---- what the box and the thread say (no app)

test('chips say a file as the Claude app does, and pictures are told apart', () => {
  expect(badgeOf({ name: 'seed-order.pdf', kind: 'pdf' })).toBe('PDF');
  expect(badgeOf({ name: 'notes.md', kind: 'markdown' })).toBe('MD');
  expect(badgeOf({ name: 'plan.docx', kind: 'word' })).toBe('DOCX');
  expect(badgeOf({ name: 'Makefile', kind: 'text' })).toBe('TXT');
  expect(badgeOf({ name: 'data.geojson', kind: 'code' })).toBe('CODE');
  expect([fileSize(572), fileSize(2150), fileSize(14_800), fileSize(1_468_006)]).toEqual(['572B', '2.1 KB', '14 KB', '1.4 MB']);
  expect(isPicturePath('C:\\Shots\\Beds.PNG')).toBe(true);
  expect(isPicturePath('C:\\Docs\\beds.pdf')).toBe(false);
  expect(filesTokens([])).toBe(0);
  expect(filesTokens([{ tokens: 100 }, { tokens: 10 }])).toBe(100 + 24 + 10 + 24 + 8);
  expect(fileTitle({ name: 'o.pdf', bytes: 2048, pages: 3, tokens: 1500 })).toBe('o.pdf · 2.0 KB · 3 pages · about 1.5k tokens of text');
  expect(fileTitle({ name: 'o.pdf', gone: true })).toContain('removed');
  expect([filesPrompt([{}]), filesPrompt([{}, {}])]).toEqual(["What's in this file?", "What's in these files?"]);
  expect(skippedLine([{ name: 'a', why: 'A.' }, { name: 'b', why: 'B.' }])).toBe('A. B.');
});

// ---- in the app

test('files dropped on Chat wait as chips; sent, their text goes before the question and the question keeps them', async ({ yap, main, fakeLlm, shot }) => {
  await freshChat(main);
  await drop(yap, [SEEDS, ORDER, BEDS, BUDGET]);
  // The two readable files: chips with their badge, name and size; the picture a screenshot tile.
  await expect(boxFiles(main)).toHaveCount(2);
  await expect(boxFiles(main).nth(0)).toContainText('TXT');
  await expect(boxFiles(main).nth(0)).toContainText('seed-list.txt');
  await expect(boxFiles(main).nth(0)).toContainText('38B');
  await expect(boxFiles(main).nth(1)).toContainText('PDF');
  await expect(boxFiles(main).nth(1)).toContainText('seed-order.pdf');
  await expect(main.getByTestId('chat-attachments').locator('.shotchip')).toHaveCount(1);
  // The spreadsheet: said plainly, not added.
  await expect(main.getByText("Chat can't read budget.xlsx (a spreadsheet: save it as CSV to attach it).", { exact: false })).toBeVisible();
  await shot(main, '01-files-in-the-box');
  // (The picture is for another test: out of the box.)
  await main.getByTestId('chat-attachments').locator('.shotchip').hover();
  await main.getByRole('button', { name: 'Remove the screenshot' }).click();
  await expect(box(main)).toHaveAttribute('placeholder', 'Ask about these files…');

  fakeLlm.scriptChat([{ content: 'Twelve packets of runner beans arrive on Friday; the list has three crops.' }, { content: 'Sweet peas are on the list.' }]);
  await box(main).fill('When does the seed order come, and what is on my list?');
  await main.getByRole('button', { name: 'Send' }).click();
  await expect(thread(main).locator('.cbubble.assistant').last()).toHaveText('Twelve packets of runner beans arrive on Friday; the list has three crops.');
  // What the model got: the documents first — the text file's lines, the PDF's text read here — then the question.
  const asked = lastAsked(fakeLlm);
  expect(asked.startsWith('<documents>\n<document index="1">\n<source>seed-list.txt</source>\n<document_content>\nRunner beans\nSweet peas\nCourgettes\n</document_content>')).toBe(true);
  expect(asked).toContain('<source>seed-order.pdf (a PDF, 1 page: its text, read on the person\'s PC)</source>');
  expect(asked).toContain('[Page 1]\nTwelve packets of runner beans, due Friday');
  expect(asked.trimEnd().endsWith('When does the seed order come, and what is on my list?')).toBe(true);
  // The box is empty; the question shows its files' chips, kept in chats.json (never their text).
  await expect(main.getByTestId('chat-files')).toHaveCount(0);
  const sent = thread(main).getByTestId('chat-files-sent').getByTestId('chat-file-chip');
  await expect(sent).toHaveCount(2);
  await expect(sent.nth(1)).toContainText('seed-order.pdf');
  await expectStore(yap, 'chats.json', (s) => {
    const q = s.conversations.flatMap((c) => c.messages).find((m) => m.attachments?.length);
    return q?.attachments.map((a) => [a.name, a.kind]).join() === 'seed-list.txt,text,seed-order.pdf,pdf' && !JSON.stringify(q).includes('Courgettes');
  });
  await shot(main, '02-a-question-with-its-files');

  // A follow-up: the earlier question's files go again in the history.
  await box(main).fill('Which flower is on it?');
  await main.getByRole('button', { name: 'Send' }).click();
  await expect(thread(main).locator('.cbubble.assistant').last()).toHaveText('Sweet peas are on the list.');
  const history = allAsked(fakeLlm);
  expect(history.some((t) => t.includes('Courgettes') && t.includes('When does the seed order come'))).toBe(true);
  expect(lastAsked(fakeLlm)).not.toContain('<documents>');

  // A chip opens its file (its app; the test run keeps what it would have opened).
  await sent.nth(0).getByRole('button', { name: /seed-list\.txt/ }).click();
  await expect.poll(() => yap.invoke('e2e_opened_urls')).toContainEqual('open-attachment:seed-list.txt');
});

test('Retry sends a question's files again, once, and the question keeps them', async ({ yap, main, fakeLlm }) => {
  await freshChat(main);
  await drop(yap, [SEEDS]);
  await expect(boxFiles(main)).toHaveCount(1);
  fakeLlm.scriptChat([{ content: 'Three crops are on it.' }]);
  await box(main).fill('How many crops are on my list?');
  await main.getByRole('button', { name: 'Send' }).click();
  const answers = thread(main).locator('.cbubble.assistant');
  await expect(answers).toHaveText(['Three crops are on it.']);
  fakeLlm.scriptChat([{ content: 'Runner beans, sweet peas and courgettes: three.' }]);
  await answers.first().hover();
  await thread(main).getByTestId('chat-retry').click();
  await expect(answers).toHaveText(['Runner beans, sweet peas and courgettes: three.']);
  // The files went again with the question, once, before its words.
  const asked = lastAsked(fakeLlm);
  expect(asked.startsWith('<documents>')).toBe(true);
  expect(asked.split('Courgettes').length - 1).toBe(1);
  expect(asked.trimEnd().endsWith('How many crops are on my list?')).toBe(true);
  await expect(thread(main).getByTestId('chat-files-sent').getByTestId('chat-file-chip')).toHaveCount(1);
});

test('✕ takes a file out of the box and off this PC; one too big for the model is said before it goes', async ({ yap, main }) => {
  await freshChat(main);
  const before = (await yap.invoke('chat_attachments_usage')).count;
  await drop(yap, [SEEDS]);
  await expect(boxFiles(main)).toHaveCount(1);
  await expect.poll(async () => (await yap.invoke('chat_attachments_usage')).count).toBe(before + 1);
  await boxFiles(main).hover();
  await main.getByRole('button', { name: 'Remove seed-list.txt' }).click();
  await expect(boxFiles(main)).toHaveCount(0);
  await expect.poll(async () => (await yap.invoke('chat_attachments_usage')).count).toBe(before);

  // Bigger than the model takes in one question: the line says so, Send waits.
  const limits = await yap.invoke('chat_limits');
  const line = 'Bed 4: watered, weeded, two courgettes picked.\n';
  fs.writeFileSync(HUGE, line.repeat(Math.ceil(((limits.questionMax + 5000) * 3.6) / line.length)));
  await drop(yap, [HUGE]);
  await expect(boxFiles(main)).toHaveCount(1);
  await box(main).fill('Summarise the log');
  await expect(main.getByTestId('chat-too-long')).toBeVisible();
  await expect(main.getByRole('button', { name: 'Send' })).toBeDisabled();
  await boxFiles(main).hover();
  await main.getByRole('button', { name: 'Remove allotment-log.txt' }).click();
  await expect(main.getByTestId('chat-too-long')).toHaveCount(0);
});

test('a chat deleted takes its files; Settings counts them and deletes them all', async ({ yap, main, fakeLlm }) => {
  await freshChat(main);
  await drop(yap, [SEEDS]);
  await expect(boxFiles(main)).toHaveCount(1);
  fakeLlm.scriptChat([{ content: 'Three crops.' }]);
  await box(main).fill('How many crops?');
  await main.getByRole('button', { name: 'Send' }).click();
  await expect(thread(main).locator('.cbubble.assistant').last()).toHaveText('Three crops.');
  const conv = await main.locator('.conv.active').getAttribute('data-conv');
  const id = (await yap.invoke('chat_get', { id: Number(conv) })).messages.find((m) => m.attachments?.length).attachments[0].id;
  const files = path.join(yap.dataDir, 'chat-files');
  expect(fs.existsSync(path.join(files, `${id}.txt`))).toBe(true);
  expect(fs.existsSync(path.join(files, id, 'seed-list.txt'))).toBe(true);
  await yap.invoke('chat_delete', { id: Number(conv) });
  await expect.poll(() => fs.existsSync(path.join(files, `${id}.txt`))).toBe(false);
  expect(fs.existsSync(path.join(files, id))).toBe(false);

  // Settings → Privacy & data → Chat files: one waiting in the box is counted, then Delete all… empties it.
  await freshChat(main);
  await drop(yap, [SEEDS]);
  await expect(boxFiles(main)).toHaveCount(1);
  const dialog = await openSettings(main, 'Privacy');
  const keep = dialog.locator('[data-setting="chat-files"]');
  await expect(keep).toContainText('Keep chat files');
  const del = dialog.locator('[data-setting="chat-files-delete"]');
  // (Earlier tests' chat's files too: this worker's.)
  await expect(del).toContainText(/\d+ files? · /);
  await del.getByRole('button', { name: 'Delete all…' }).click();
  const confirm = main.getByRole('alertdialog', { name: 'Delete all chat files?' });
  await confirm.getByRole('button', { name: 'Delete files' }).click();
  await expect(del).toContainText('None kept right now');
  await closeSettings(main);
  expect((await yap.invoke('chat_attachments_usage')).count).toBe(0);
});
