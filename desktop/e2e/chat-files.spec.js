// Files attached to a question (desktop/src-tauri/src/chat/attachments.rs, 15 Nov 2026; the Claude app's + and drop):
// - A text file and a PDF dropped on Chat wait in the box as chips — a type badge, the name and the size, as the
//   Claude app's — beside a picture dropped with them (that one a screenshot tile). A file Chat can't read is said
//   plainly, never added.
// - Sent: their text goes before the question (a <documents> block; the PDF's read on this PC), the question keeps
//   their chips (chats.json `attachments`), a follow-up sends them again in the history, a chip opens its file.
// - ✕ takes one out and deletes it; one too big for the model is said before it's sent; deleting the chat deletes
//   its files; Settings → Privacy & data → Chat files counts them and deletes them all.
// - Wider reading (16 Nov 2026): a PDF's scanned page read with OCR (its chip says "OCR"; the page drawn by Windows'
//   own PDF renderer, the OCR's text a fake here, as the snip specs': `e2e_snip_ocr`), a PowerPoint presentation slide
//   by slide with its speaker notes, an older Word document (.doc, made by LibreOffice: a fixture); a broken .pptx, a
//   broken .doc and an older .ppt said plainly, each.
//
// The model is a local fake (support/fake-llm.js). Fictional files only.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { test as base, expect, expectStore, openView, openSettings, closeSettings } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { writePng } from './support/png.js';
import { zip } from './support/docx.js';
import { badgeOf, fileSize, filesTokens, isPicturePath, fileTitle, filesPrompt, skippedLine, ocrLine } from '../src/lib/views/chat/chatFiles.js';

const HERE = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', '..', 'test-results', 'chat-files');
const FIXTURES = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', 'src-tauri', 'src', 'chat', 'attachments', 'fixtures');
const SEEDS = path.join(HERE, 'seed-list.txt');
const ORDER = path.join(HERE, 'seed-order.pdf');
const BEDS = path.join(HERE, 'beds.png');
const BUDGET = path.join(HERE, 'budget.xlsx');
const HUGE = path.join(HERE, 'allotment-log.txt');
const NOTICE = path.join(HERE, 'allotment-notice.pdf');
const DECK = path.join(HERE, 'allotment-year.pptx');
const RULES = path.join(HERE, 'allotment-rules.doc');
const OLD_DECK = path.join(HERE, 'old-slides.ppt');
const BROKEN_DECK = path.join(HERE, 'broken-deck.pptx');
const BROKEN_DOC = path.join(HERE, 'broken-rules.doc');

/** The scanned page's picture (fixtures/scanned-note.png: fictional words, 8-bit grey): its size and compressed rows,
 *  which a PDF's FlateDecode with PNG's predictors takes as they are. */
function scanPicture() {
  const png = fs.readFileSync(path.join(FIXTURES, 'scanned-note.png'));
  let w = 0;
  let h = 0;
  const idat = [];
  for (let at = 8; at + 8 <= png.length; ) {
    const len = png.readUInt32BE(at);
    const kind = png.toString('latin1', at + 4, at + 8);
    const data = png.subarray(at + 8, at + 8 + len);
    if (kind === 'IHDR') [w, h] = [data.readUInt32BE(0), data.readUInt32BE(4)];
    if (kind === 'IDAT') idat.push(data);
    at += 12 + len;
  }
  return { w, h, rows: Buffer.concat(idat) };
}

/** A PDF of `pages` (`{ text }` typed, `{ scan: true }` a picture of words and no text), its cross-reference table
 *  right (made here: nobody's file). */
function pdfOf(pages) {
  const { w, h, rows } = scanPicture();
  const esc = (t) => t.replace(/[\\()]/g, (c) => '\\' + c);
  const kids = pages.map((_, i) => `${6 + i * 2} 0 R`).join(' ');
  const objects = [
    Buffer.from('<< /Type /Catalog /Pages 2 0 R >>'),
    Buffer.from(`<< /Type /Pages /Kids [${kids}] /Count ${pages.length} >>`),
    Buffer.from('<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>'),
    Buffer.concat([
      Buffer.from(`<< /Type /XObject /Subtype /Image /Width ${w} /Height ${h} /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode /DecodeParms << /Predictor 15 /Colors 1 /BitsPerComponent 8 /Columns ${w} >> /Length ${rows.length} >>\nstream\n`),
      rows,
      Buffer.from('\nendstream'),
    ]),
  ];
  for (const p of pages) {
    const content = p.scan ? 'q 500 0 0 150 56 600 cm /Im1 Do Q' : `BT /F1 12 Tf 72 720 Td (${esc(p.text)}) Tj ET`;
    objects.push(Buffer.from(`<< /Length ${content.length} >>\nstream\n${content}\nendstream`));
    objects.push(Buffer.from(`<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents ${objects.length} 0 R /Resources << /Font << /F1 3 0 R >> /XObject << /Im1 4 0 R >> >> >>`));
  }
  const parts = [Buffer.from('%PDF-1.4\n')];
  let size = parts[0].length;
  const offsets = [];
  objects.forEach((o, i) => {
    offsets.push(size);
    const obj = Buffer.concat([Buffer.from(`${i + 1} 0 obj\n`), o, Buffer.from('\nendobj\n')]);
    parts.push(obj);
    size += obj.length;
  });
  let tail = `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n`;
  for (const off of offsets) tail += `${String(off).padStart(10, '0')} 00000 n \n`;
  tail += `trailer\n<< /Size ${objects.length + 1} /Root 1 0 R >>\nstartxref\n${size}\n%%EOF\n`;
  return Buffer.concat([...parts, Buffer.from(tail)]);
}

/** A one-page PDF with `text` on it. */
const tinyPdf = (text) => pdfOf([{ text }]);

/** A PowerPoint presentation made here: two slides (listed in the opposite order to their numbers), the second with
 *  speaker notes. */
function deck() {
  const sp = (type, text) => `<p:sp><p:nvSpPr><p:cNvPr id="2" name="${type}"/><p:cNvSpPr/><p:nvPr><p:ph type="${type}"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:p><a:r><a:t>${text}</a:t></a:r></a:p></p:txBody></p:sp>`;
  const slide = (...shapes) => `<?xml version="1.0" encoding="UTF-8"?><p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree>${shapes.join('')}</p:spTree></p:cSld></p:sld>`;
  const rel = (id, type, target) => `<Relationship Id="${id}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/${type}" Target="${target}"/>`;
  const rels = (...r) => `<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">${r.join('')}</Relationships>`;
  return zip({
    '[Content_Types].xml': '<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"/>',
    'ppt/presentation.xml': '<?xml version="1.0" encoding="UTF-8"?><p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:sldIdLst><p:sldId id="256" r:id="rId3"/><p:sldId id="257" r:id="rId2"/></p:sldIdLst></p:presentation>',
    'ppt/_rels/presentation.xml.rels': rels(rel('rId2', 'slide', 'slides/slide1.xml'), rel('rId3', 'slide', 'slides/slide2.xml')),
    'ppt/slides/slide1.xml': slide(sp('title', 'Spring planting'), sp('body', 'Beans along the north fence')),
    'ppt/slides/_rels/slide1.xml.rels': rels(rel('rId2', 'notesSlide', '../notesSlides/notesSlide1.xml')),
    'ppt/notesSlides/notesSlide1.xml': slide(sp('sldImg', ''), sp('body', 'Mention the frost dates.'), sp('sldNum', '2')).replace(/p:sld\b/g, 'p:notes'),
    'ppt/slides/slide2.xml': slide(sp('ctrTitle', 'Allotment year')),
  });
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
      fs.writeFileSync(NOTICE, pdfOf([{ text: 'Seed order for the spring' }, { scan: true }]));
      fs.writeFileSync(DECK, deck());
      fs.copyFileSync(path.join(FIXTURES, 'allotment-rules.doc'), RULES);
      // An older .ppt (its contents don't matter: never read), a .pptx that isn't a zip, a .doc cut short.
      fs.writeFileSync(OLD_DECK, Buffer.from([0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1, 0, 0, 0, 0]));
      fs.writeFileSync(BROKEN_DECK, 'Not a presentation at all');
      fs.writeFileSync(BROKEN_DOC, fs.readFileSync(path.join(FIXTURES, 'allotment-rules.doc')).subarray(0, 700));
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
  // Wider reading: a presentation's badge; a scan's pages read with OCR, and any left unread.
  expect(badgeOf({ name: 'year.pptx', kind: 'powerpoint' })).toBe('PPTX');
  expect(badgeOf({ name: 'year', kind: 'powerpoint' })).toBe('PPT');
  expect(ocrLine({ ocrPages: 1 })).toBe('1 page read with OCR');
  expect(ocrLine({ ocrPages: 20, unreadPages: 28 })).toBe('20 pages read with OCR, 28 not read');
  expect(ocrLine({ unreadPages: 2 })).toBe('2 pages with no text not read');
  expect(ocrLine({ pages: 3 })).toBe('');
  expect(fileTitle({ name: 'n.pdf', bytes: 2048, pages: 2, ocrPages: 1, tokens: 40 })).toBe('n.pdf · 2.0 KB · 2 pages · 1 page read with OCR · about 40 tokens of text');
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
  // A broken spreadsheet: said plainly, not added (a whole one is read: chat-sheets.spec.js).
  await expect(main.getByText("Yap couldn't read the cells of budget.xlsx.", { exact: false })).toBeVisible();
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

test("a scanned page is read with OCR, a presentation slide by slide with its notes, an older Word document's text; what can't be read is said", async ({ yap, main, fakeLlm, shot }) => {
  // The OCR's words (Windows' own languages differ per PC); the page itself is drawn by Windows' PDF renderer.
  await yap.invoke('e2e_snip_ocr', { text: 'Compost the beds on Tuesday.\nBring twelve bags of mulch.' });
  try {
    await freshChat(main);
    await drop(yap, [NOTICE, DECK, RULES, OLD_DECK, BROKEN_DECK, BROKEN_DOC]);
    await expect(boxFiles(main)).toHaveCount(3);
    await expect(boxFiles(main).nth(0)).toContainText('PDF');
    await expect(boxFiles(main).nth(0)).toContainText('allotment-notice.pdf');
    // The scan's chip says it was read with OCR (its tooltip how many pages).
    await expect(boxFiles(main).nth(0).getByTestId('chat-file-ocr')).toHaveText('OCR');
    await expect(boxFiles(main).nth(0).getByRole('button', { name: /allotment-notice\.pdf, .*1 page read with OCR/ })).toHaveAttribute('title', /2 pages · 1 page read with OCR/);
    await expect(boxFiles(main).nth(1)).toContainText('PPTX');
    await expect(boxFiles(main).nth(1).getByTestId('chat-file-ocr')).toHaveCount(0);
    await expect(boxFiles(main).nth(2)).toContainText('DOC');
    await expect(boxFiles(main).nth(2)).toContainText('allotment-rules.doc');
    // What can't be read: each said plainly, worded for its kind.
    await expect(main.getByText('Chat can\'t read old-slides.ppt (an older PowerPoint presentation: save it as .pptx in PowerPoint to attach it).', { exact: false })).toBeVisible();
    await expect(main.getByText("Yap couldn't read the text of broken-deck.pptx.", { exact: false })).toBeVisible();
    await expect(main.getByText("Yap couldn't read the text of broken-rules.doc (it may be from Word 95 or older): save it as .docx in Word to attach it.", { exact: false })).toBeVisible();
    await shot(main, '03-a-scan-a-presentation-and-an-older-word-document');

    fakeLlm.scriptChat([{ content: 'Compost on Tuesday; the year starts with spring planting.' }]);
    await box(main).fill('What do these say?');
    await main.getByRole('button', { name: 'Send' }).click();
    await expect(thread(main).locator('.cbubble.assistant').last()).toHaveText('Compost on Tuesday; the year starts with spring planting.');
    const asked = lastAsked(fakeLlm);
    // The PDF: its typed page, then its scanned page as the OCR read it, marked so.
    expect(asked).toContain("<source>allotment-notice.pdf (a PDF, 2 pages: its text, read on the person's PC; 1 scanned page read with OCR, which can get words wrong)</source>");
    expect(asked).toContain('[Page 1]\nSeed order for the spring\n\n[Page 2, read with OCR]\nCompost the beds on Tuesday.\nBring twelve bags of mulch.\n</document_content>');
    // The presentation: in its own order, a slide's title first, its notes after.
    expect(asked).toContain("<source>allotment-year.pptx (a PowerPoint presentation: each slide's text, then its speaker notes)</source>");
    expect(asked).toContain('Slide 1: Allotment year\n\nSlide 2: Spring planting\nBeans along the north fence\nSpeaker notes: Mention the frost dates.\n</document_content>');
    // The older Word document: its paragraphs, its table's rows, a link's words without its code.
    expect(asked).toContain('<source>allotment-rules.doc (a Word document: its text)</source>');
    expect(asked).toContain('Keep the paths clear – always. Water before nine in the morning.\nSee the site map by the gate.\nPlot\tHolder\n7\tSam Example');
    expect(asked).not.toContain('HYPERLINK');
    expect(asked.trimEnd().endsWith('What do these say?')).toBe(true);
    // The question keeps them, the scan's pages read with OCR with it; its chip says so in the thread too.
    await expectStore(yap, 'chats.json', (s) => {
      const q = s.conversations.flatMap((c) => c.messages).filter((m) => m.attachments?.length).at(-1);
      return q?.attachments.map((a) => `${a.kind}:${a.ocrPages || 0}`).join() === 'pdf:1,powerpoint:0,word:0';
    });
    await expect(thread(main).getByTestId('chat-files-sent').last().getByTestId('chat-file-ocr')).toHaveCount(1);
  } finally {
    await yap.invoke('e2e_snip_ocr', { text: null });
  }
});

// Asked out loud (the Voice Assistant, 16 Nov 2026): with Chat in view the box's files go with the question, as a typed
// one takes them; asked from another app they stay for the box, as a quoted passage does.
test('asked out loud with Chat in view, a question takes the files in the box; from another app they stay', async ({ yap, main, fakeLlm, shot }) => {
  const askAloud = (text) => yap.invoke('e2e_assistant_ask', { text, listenMs: 0 });
  await freshChat(main);
  await drop(yap, [SEEDS]);
  await expect(boxFiles(main)).toHaveCount(1);

  // From another app (Yap's window not in view): the question is about something else, and the file stays.
  await yap.invoke('e2e_focus', { focused: false });
  try {
    fakeLlm.scriptChat([{ content: 'Sunny all week.' }]);
    await askAloud('What is the weather like');
    await expect(thread(main)).toContainText('Sunny all week.', { timeout: 20_000 });
  } finally {
    await yap.invoke('e2e_focus', { focused: true });
  }
  expect(lastAsked(fakeLlm)).not.toContain('<documents>');
  expect(lastAsked(fakeLlm)).not.toContain('Runner beans');
  await expect(boxFiles(main)).toHaveCount(1);
  await expect(thread(main).getByTestId('chat-files-sent')).toHaveCount(0);

  // With Chat in front, on the chat the box is in: the file goes with the spoken question, and its chip goes.
  fakeLlm.scriptChat([{ content: 'Runner beans, sweet peas and courgettes.' }]);
  await askAloud("What's on my seed list?");
  await expect(thread(main).locator('.cbubble.assistant').last()).toHaveText('Runner beans, sweet peas and courgettes.', { timeout: 20_000 });
  const asked = lastAsked(fakeLlm);
  expect(asked.startsWith('<documents>\n<document index="1">\n<source>seed-list.txt</source>\n<document_content>\nRunner beans\nSweet peas\nCourgettes\n</document_content>')).toBe(true);
  expect(asked).toContain("What's on my seed list?");
  await expect(main.getByTestId('chat-files')).toHaveCount(0);
  const sent = thread(main).getByTestId('chat-files-sent').getByTestId('chat-file-chip');
  await expect(sent).toHaveCount(1);
  await expect(sent).toContainText('seed-list.txt');
  // Kept with the question, asked out loud (never its text).
  await expectStore(yap, 'chats.json', (s) => {
    const q = s.conversations.flatMap((c) => c.messages).find((m) => m.role === 'user' && m.text === "What's on my seed list?");
    return q?.voice === true && q.attachments?.map((a) => a.name).join() === 'seed-list.txt' && !JSON.stringify(q).includes('Courgettes');
  });
  // (Nor was the window in front looked at: the file is what it's about.)
  expect(fakeLlm.requests.some((r) => r.kind === 'screenQuestion' && r.user.includes('seed list'))).toBe(false);
  await shot(main, '04-a-spoken-question-with-its-file');

  // A follow-up out loud takes nothing more: the box is empty.
  fakeLlm.scriptChat([{ content: 'Sweet peas.' }]);
  await askAloud('Which one is a flower?');
  await expect(thread(main).locator('.cbubble.assistant').last()).toHaveText('Sweet peas.', { timeout: 20_000 });
  expect(lastAsked(fakeLlm)).not.toContain('<documents>');
});

test('a long scan shows "Reading…" while its pages are read; past 20 pages, the rest are named', async ({ yap, main }) => {
  const LONG = path.join(HERE, 'allotment-minutes.pdf');
  fs.writeFileSync(LONG, pdfOf(Array.from({ length: 21 }, () => ({ scan: true }))));
  await yap.invoke('e2e_snip_ocr', { text: 'Minutes of the plot holders.' });
  try {
    await freshChat(main);
    await drop(yap, [LONG]);
    // Windows draws 20 pages: seconds, so the chip says it's reading meanwhile.
    await expect(main.getByTestId('chat-file-reading')).toContainText('Reading…');
    await expect(boxFiles(main)).toHaveCount(1, { timeout: 60_000 });
    await expect(main.getByTestId('chat-file-reading')).toHaveCount(0);
    await expect(boxFiles(main).getByRole('button', { name: /allotment-minutes\.pdf, / })).toHaveAttribute('title', /21 pages · 20 pages read with OCR, 1 not read/);
    // Its text ends with the note naming the page left out.
    const files = path.join(yap.dataDir, 'chat-files');
    const texts = fs.readdirSync(files).filter((f) => f.endsWith('.txt')).map((f) => fs.readFileSync(path.join(files, f), 'utf8'));
    expect(texts.some((t) => t.includes('[Page 20, read with OCR]\nMinutes of the plot holders.') && t.endsWith('[Page 21 has no text layer (a scan or a picture) and wasn\'t read: Yap reads at most 20 such pages of a file with OCR.]'))).toBe(true);
  } finally {
    await yap.invoke('e2e_snip_ocr', { text: null });
  }
});

test("Edit sends a question's files again, once, and the question keeps them", async ({ yap, main, fakeLlm }) => {
  await freshChat(main);
  await drop(yap, [SEEDS]);
  await expect(boxFiles(main)).toHaveCount(1);
  fakeLlm.scriptChat([{ content: 'Three crops are on it.' }]);
  await box(main).fill('How many crops are on my list?');
  await main.getByRole('button', { name: 'Send' }).click();
  const answers = thread(main).locator('.cbubble.assistant');
  await expect(answers).toHaveText(['Three crops are on it.']);
  fakeLlm.scriptChat([{ content: 'Runner beans, sweet peas and courgettes: three.' }]);
  await thread(main).locator('.cbubble.user').first().hover();
  await thread(main).getByTestId('chat-edit-message').first().click();
  await thread(main).getByTestId('chat-edit-input').fill('Which crops are on my list?');
  await thread(main).getByTestId('chat-edit-save').click();
  await expect(answers).toHaveText(['Runner beans, sweet peas and courgettes: three.']);
  // The files went again with the question, once, before its words.
  const asked = lastAsked(fakeLlm);
  expect(asked.startsWith('<documents>')).toBe(true);
  expect(asked.split('Courgettes').length - 1).toBe(1);
  expect(asked.trimEnd().endsWith('Which crops are on my list?')).toBe(true);
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
