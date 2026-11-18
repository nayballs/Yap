// Projects (desktop/src-tauri/src/chat/projects.rs; views/chat/ProjectsView.svelte, chatState's
// `page`; 15 Nov 2026, the Claude app's projects from Anthropic's help pages): a named group of
// chats sharing instructions and files that every chat in it gets.
//
// - Projects in the conversation list opens the page; New project makes one (a name, what it's
//   for) and opens it. Its instructions save as typed; a file added is read with the attachments'
//   rules (a Markdown file kept, a spreadsheet said plainly and left out).
// - New chat in it: the band says so; the question's system prompt carries the project's name,
//   instructions and the file's text; the chat is listed on the project's page.
// - A chat's ⋯ → Move to project ▸ puts another chat in; Remove from project takes it out.
// - Deleted (asked first): its chats stay, out of it. Light and dark.
// - Nothing in chats.json: projects.json, a file an older Yap never touches.
// - **A big project** (chat/projects/knowledge.rs, 16 Nov 2026, the Claude app's project
//   knowledge): files too long for the model (the fake's 8K) aren't sent whole. The system
//   prompt lists them and their parts; the question gets the part that matches it, its
//   <source> naming the file and its section; the model reads another with
//   read_project_file; the answer's steps say which parts were used; the project's page says
//   how the files are read.
// - **Its parts as numbered sources** (chat/projects/sources.rs, 16 Nov 2026, the Claude app's citations to project
//   files): the parts sent with the question are [1]…, a part a tool reads the next number; the answer's "[2]" is a
//   pill naming the file and its place, and its row says "From the project". A pill opens the file beside the chat:
//   the PDF as Yap read it at its page, the Markdown at its heading (ProjectFileTab), the spreadsheet in the sheet
//   view at its row.
//
// The model is the scripted fake (support/fake-llm.js). Every detail is fictional.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, setConfig } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { RUNS } from './support/yap.js';
import { chipKind, stepsSummary } from '../src/lib/views/chat/chatSteps.js';
import { knowledgeLine } from '../src/lib/views/chat/projectLines.js';
import { blockLines, blocksOf, drawnAs, inPart, partView, placeLabel } from '../src/lib/views/chat/projectFile.js';

const HERE = path.join(RUNS, 'chat-projects-spec');
const SOWING = path.join(HERE, 'sowing-plan.md');
const SHEET = path.join(HERE, 'seed-costs.xlsx');
const HANDBOOK = path.join(HERE, 'staff-handbook.md');
const FACILITIES = path.join(HERE, 'facilities-handbook.pdf');
const SEATS = path.join(HERE, 'seat-plan.csv');

/** A PDF whose pages are `pages` (a line of text each), its cross-reference table right (made here: nobody's file). */
function textPdf(pages) {
  const esc = (t) => t.replace(/[\\()]/g, (c) => '\\' + c);
  // (Each page's contents, then the page: objects 4 and 5, 6 and 7…)
  const kids = pages.map((_, i) => `${5 + i * 2} 0 R`).join(' ');
  const objects = [Buffer.from('<< /Type /Catalog /Pages 2 0 R >>'), Buffer.from(`<< /Type /Pages /Kids [${kids}] /Count ${pages.length} >>`), Buffer.from('<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>')];
  for (const text of pages) {
    const content = `BT /F1 10 Tf 40 760 Td (${esc(text)}) Tj ET`;
    objects.push(Buffer.from(`<< /Length ${content.length} >>\nstream\n${content}\nendstream`));
    objects.push(Buffer.from(`<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents ${objects.length} 0 R /Resources << /Font << /F1 3 0 R >> >> >>`));
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

/** A fictional facilities handbook as a PDF: 40 pages, page 23 the only one about the bike shed. */
function facilities() {
  const pages = [];
  for (let p = 1; p <= 40; p++) {
    let text = '';
    while (text.split(/\s+/).length < 190) text += `The facilities policy at Example Ltd covers every member of staff on floor ${p}, and the office manager checks it each month. `;
    if (p === 23) text += 'The Kellbrook bike shed is behind the north gate, and its code is 4417.';
    pages.push(text);
  }
  return textPdf(pages);
}

/** A fictional seat plan: 120 seats, one of them in the Kellbrook room. */
function seats() {
  let s = 'Seat,Floor,Wing\n';
  for (let r = 2; r <= 121; r++) s += `Seat ${r},Floor ${1 + (r % 4)},${r === 88 ? 'Kellbrook room' : 'North wing'}\n`;
  return s;
}

/** A fictional staff handbook in Markdown, far too long for an 8K model: 40 sections, one the leave policy. */
function handbook() {
  const topics = ['Working hours', 'Expenses', 'Annual leave', 'Sick pay', 'Parking', 'Training', 'Security', 'Equipment', 'Travel', 'Pensions'];
  let s = '# Example Ltd staff handbook\n\n';
  for (let i = 0; i < 40; i++) {
    const topic = topics[i % topics.length];
    s += `## ${i + 1}. ${topic}${i >= topics.length ? ` (${Math.floor(i / topics.length) + 1})` : ''}\n\n`;
    let para = '';
    while (para.split(/\s+/).length < 300) para += `The ${topic.toLowerCase()} policy at Example Ltd covers every member of staff, and line managers check it each quarter. `;
    s += para;
    if (i === 2) s += 'Full-time staff get 27 days of annual leave a year plus bank holidays, and up to five unused days carry over.';
    s += '\n\n';
  }
  return s;
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
      fs.writeFileSync(SOWING, '# Sowing plan\n\nBroad beans go in on 3 March, in bed 4.\n');
      fs.writeFileSync(SHEET, Buffer.from([0x50, 0x4b, 0x03, 0x04, 0, 0, 0, 0]));
      fs.writeFileSync(HANDBOOK, handbook());
      fs.writeFileSync(FACILITIES, facilities());
      fs.writeFileSync(SEATS, seats());
      await use({
        name: 'chat-projects',
        config: {
          postProcessEnabled: false,
          meetingReminder: 'never',
          webSearch: false,
          speakReplies: false,
          llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat', apiKey: '', apiKeys: {}, prompt: '', disableThinking: false } },
        },
        data: {
          'chats.json': {
            conversations: [
              { id: 1, title: 'Greenhouse heater', createdTs: 1, updatedTs: Math.floor(Date.now() / 1000) - 60, messages: [{ role: 'user', text: 'Which heater?', ts: 1 }, { role: 'assistant', text: 'A small paraffin one.', ts: 2 }] },
            ],
          },
        },
      });
    },
    { scope: 'worker' },
  ],
});

const box = (main) => main.getByRole('textbox', { name: 'Message' });
const page = (main) => main.getByTestId('chat-project');
const askedWith = (fakeLlm, text) => fakeLlm.requests.filter((r) => Array.isArray(r.messages) && JSON.stringify(r.messages).includes(text) && r.kind === 'other').at(-1);
const systemOf = (req) => String(req?.messages?.find((m) => m.role === 'system')?.content ?? '');

async function theme(yap, main, which) {
  await setConfig(yap, main, { theme: which });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe(which);
}

test('a project: made, its instructions and files, a chat in it carrying them, and deleted', async ({ yap, main, fakeLlm, shot }) => {
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'Projects', exact: true }).click();
  await expect(main.getByTestId('chat-projects')).toContainText('No projects yet.');
  await main.getByRole('button', { name: 'New project' }).click();
  await main.getByRole('textbox', { name: 'Name' }).fill('Allotment 2027');
  await main.getByRole('textbox', { name: /What it's for/ }).fill('Planning the plot for next year');
  await main.getByRole('button', { name: 'Create project' }).click();
  await expect(page(main).getByRole('heading', { name: 'Allotment 2027' })).toBeVisible();

  // Instructions save as typed.
  await page(main).getByRole('textbox', { name: 'Instructions' }).fill('Use metric units. Bed numbers are on the plot map.');
  await expect(page(main).getByText('Saved')).toBeVisible();
  const [p] = yap.readJson('projects.json').projects;
  expect(p.instructions).toBe('Use metric units. Bed numbers are on the plot map.');

  // Files, read as attachments are: the Markdown kept, the spreadsheet said and left out.
  const out = await yap.invoke('project_files_add', { id: p.id, paths: [SOWING, SHEET] });
  expect(out.added.map((f) => f.name)).toEqual(['sowing-plan.md']);
  expect(out.skipped.map((s) => s.name)).toEqual(['seed-costs.xlsx']);
  await main.getByRole('button', { name: 'Projects', exact: true }).click();
  await main.getByRole('button', { name: /Allotment 2027/ }).click();
  await expect(page(main).getByRole('list', { name: 'Files in Allotment 2027' })).toContainText('sowing-plan.md');
  await shot(main, '01-project-page');
  await theme(yap, main, 'dark');
  await shot(main, '02-project-page-dark');
  await theme(yap, main, 'light');

  // A chat in it: the band, then its question carries the project.
  await page(main).getByRole('button', { name: 'New chat in Allotment 2027' }).click();
  await expect(main.getByTestId('chat-project-line')).toContainText('Allotment 2027');
  fakeLlm.scriptChat([{ content: 'On 3 March, in bed 4.' }]);
  await box(main).fill('When do the broad beans go in?');
  await main.getByRole('button', { name: 'Send' }).click();
  await expect(main.locator('.cbubble.assistant').last()).toContainText('bed 4');
  const sys = systemOf(askedWith(fakeLlm, 'When do the broad beans go in?'));
  expect(sys).toContain('This chat is in the user\'s project "Allotment 2027". It\'s for: Planning the plot for next year');
  expect(sys).toContain('<project_instructions>\nUse metric units. Bed numbers are on the plot map.\n</project_instructions>');
  expect(sys).toContain('<source>sowing-plan.md</source>');
  expect(sys).toContain('Broad beans go in on 3 March, in bed 4.');
  await shot(main, '03-chat-in-project');
  expect(JSON.stringify(yap.readJson('chats.json'))).not.toContain('Allotment');

  // Another chat moved in from its ⋯, then out again.
  await main.locator('.conv').filter({ hasText: 'Greenhouse heater' }).click({ button: 'right' });
  const menu = main.getByRole('menu').first();
  await menu.getByRole('menuitem', { name: 'Move to project' }).click();
  await main.getByRole('menu').last().getByRole('menuitemradio', { name: 'Allotment 2027' }).click();
  await expect.poll(() => yap.readJson('projects.json').chats['1']).toBe(p.id);
  await expect(main.getByTestId('chat-project-line')).toContainText('Allotment 2027');

  // The project's page lists both chats.
  await main.getByTestId('chat-project-line').getByRole('button').click();
  await expect(page(main).getByRole('list', { name: 'Chats in Allotment 2027' }).getByRole('listitem')).toHaveCount(2);

  // Deleted, asked first: its chats stay, out of it.
  await page(main).getByRole('button', { name: 'More for Allotment 2027' }).click();
  await main.getByRole('menu').first().getByRole('menuitem', { name: 'Delete project…' }).click();
  const confirm = main.getByRole('alertdialog');
  await expect(confirm).toHaveAccessibleName('Delete “Allotment 2027”?');
  await confirm.getByRole('button', { name: 'Delete' }).click();
  await expect(main.getByTestId('chat-projects')).toContainText('No projects yet.');
  expect(yap.readJson('projects.json').projects).toEqual([]);
  expect((await yap.invoke('chats_list')).map((c) => c.title)).toEqual(expect.arrayContaining(['Greenhouse heater', 'When do the broad beans go in?']));
});

test('instructions typed while a save is on its way are saved too', async ({ yap, main }) => {
  // 18 Nov 2026: a save that came back marked the instructions saved
  // whatever was typed while it was on its way; that typing's own save then
  // found nothing to do, and it never reached projects.json. The save is
  // held here (the page's IPC request, as a busy PC holds it).
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'Projects', exact: true }).click();
  await main.getByRole('button', { name: 'New project' }).click();
  await main.getByRole('textbox', { name: 'Name' }).fill('Orchard 2027');
  await main.getByRole('button', { name: 'Create project' }).click();
  await expect(page(main).getByRole('heading', { name: 'Orchard 2027' })).toBeVisible();
  const p = yap.readJson('projects.json').projects.find((x) => x.name === 'Orchard 2027');
  await main.evaluate(() => {
    const real = window.fetch;
    window.__held = { real, hold: 'project_update', waiting: [] };
    window.fetch = function (input, init) {
      const h = window.__held;
      if (h?.hold && String(input?.url ?? input).includes(h.hold)) return new Promise((go) => h.waiting.push(go)).then(() => real.call(this, input, init));
      return real.call(this, input, init);
    };
  });
  try {
    const instructions = page(main).getByRole('textbox', { name: 'Instructions' });
    await instructions.fill('Prune in winter.');
    await expect.poll(() => main.evaluate(() => window.__held.waiting.length)).toBe(1);
    // More typed while that save is on its way, then the save comes back.
    await main.evaluate(() => (window.__held.hold = null));
    await main.keyboard.insertText(' Mulch in spring.');
    await main.evaluate(() => window.__held.waiting.splice(0).forEach((go) => go()));
    await expect
      .poll(() => yap.readJson('projects.json').projects.find((x) => x.id === p.id)?.instructions, { timeout: 5_000 })
      .toBe('Prune in winter. Mulch in spring.');
    await expect(page(main).getByText('Saved')).toBeVisible();
  } finally {
    await main.evaluate(() => {
      if (window.__held) window.fetch = window.__held.real;
      window.__held?.waiting.splice(0).forEach((go) => go());
      window.__held = null;
    });
    await yap.invoke('project_delete', { id: p.id });
    await main.getByRole('button', { name: 'Projects', exact: true }).click();
  }
});

test("a big project's steps fold into one line, and its page's line says how the files are read", () => {
  const used = 'Used from the project: staff-handbook.md, part 5, “3. Annual leave”';
  expect(stepsSummary([used])).toBe("Used a part of the project's files");
  expect(stepsSummary([used, 'Used from the project: staff-handbook.pdf, pages 23–24, “3. Annual leave”', 'Read from the project: staff-handbook.md, part 7, “4. Sick pay”', "Searched the project's files: “carry over”"])).toBe(
    "Used 2 parts of the project's files, read another part, searched the project's files"
  );
  expect(knowledgeLine({ tokens: 18_400, searched: true, model: 'Fake Chat' }, 1)).toBe('About 18K tokens: too much to send Fake Chat whole with every question, so each question gets the parts of these files that match it.');
  expect(knowledgeLine({ tokens: 900, searched: false }, 2)).toBe('About 900 tokens: every chat here reads them whole.');
  expect(knowledgeLine(null, 2)).toBe('');
  // A document's mark, never the web's ("Read …" is a page's otherwise).
  expect(chipKind('Read from the project: staff-handbook.md, part 7, “4. Sick pay”')).toBe('doc');
  expect(chipKind(used)).toBe('doc');
});

test('a big project: its files searched in parts, the matching part sent, another read with the tool, the parts said under the answer', async ({ yap, main, fakeLlm, shot }) => {
  await openView(main, 'Chat');
  const p = await yap.invoke('project_create', { name: 'People team', description: 'HR questions' });
  const out = await yap.invoke('project_files_add', { id: p.id, paths: [HANDBOOK] });
  expect(out.added.map((f) => f.name)).toEqual(['staff-handbook.md']);
  expect(out.added[0].tokens).toBeGreaterThan(10_000);

  // The page says the files are read in parts for this model (the fake's 8K).
  await main.getByRole('button', { name: 'Projects', exact: true }).click();
  await main.getByRole('button', { name: /People team/ }).click();
  await expect(page(main).getByTestId('project-knowledge')).toContainText('so each question gets the parts of these files that match it');
  await shot(main, '04-big-project-page');

  // A chat in it: the model reads part 2, then answers.
  await page(main).getByRole('button', { name: 'New chat in People team' }).click();
  fakeLlm.scriptChat([{ tools: [{ name: 'read_project_file', args: { file: 'staff-handbook.md', part: 2 } }] }, { content: 'Full-time staff get 27 days a year (staff-handbook.md, section “3. Annual leave”).' }]);
  const question = 'How many days of annual leave do I get?';
  await box(main).fill(question);
  await main.getByRole('button', { name: 'Send' }).click();
  await expect(main.locator('.cbubble.assistant').last()).toContainText('27 days a year');

  const asked = fakeLlm.requests.filter((r) => Array.isArray(r.messages) && JSON.stringify(r.messages).includes(question) && r.kind === 'other');
  const first = asked[0];
  const sys = systemOf(first);
  // The system prompt: the files listed with their parts, never their text.
  expect(sys).toContain('This chat is in the user\'s project "People team".');
  expect(sys).toContain('too long to send whole');
  expect(sys).toMatch(/<project_files>\n- staff-handbook\.md: \d+ parts: 1 section “1\. Working hours”; 2 /);
  expect(sys).not.toContain('27 days of annual leave');
  // The question: the part that answers it, its source naming the file and its section.
  const q = JSON.stringify(first.messages.filter((m) => m.role === 'user').at(-1));
  expect(q).toContain("From the project's files, the parts that match this question (read_project_file reads any other part)");
  expect(q).toContain('27 days of annual leave');
  expect(q).toMatch(/<source>staff-handbook\.md: part \d+ of \d+, section “3\. Annual leave”<\/source>/);
  // The tools: offered, and the part asked for read.
  expect(first.tools).toEqual(expect.arrayContaining(['read_project_file', 'search_project_files']));
  const results = asked.at(-1).messages.filter((m) => m.role === 'tool').map((m) => JSON.parse(m.content));
  expect(results[0].data.part).toBe(2);
  expect(results[0].data.source).toMatch(/^staff-handbook\.md: part 2 of \d+, section “/);

  // Under the answer: which parts were used, and the one it read.
  const steps = main.locator('.msgs').getByTestId('chat-steps').last();
  await expect(steps).toContainText("Used a part of the project's files, read another part");
  await steps.locator('summary').click();
  await expect(steps.getByRole('list', { name: 'Steps' })).toContainText(/Used from the project: staff-handbook\.md, part \d+, “3\. Annual leave”/);
  await expect(steps.getByRole('list', { name: 'Steps' })).toContainText('Read from the project: staff-handbook.md, part 2, “');
  await shot(main, '05-big-project-answer');
  await theme(yap, main, 'dark');
  await shot(main, '06-big-project-answer-dark');
  await theme(yap, main, 'light');
  const saved = (await yap.invoke('chats_list')).find((c) => c.title.startsWith('How many days'));
  expect(saved).toBeTruthy();
  await yap.invoke('project_delete', { id: p.id });
});

test('the project file view draws each kind as Yap read it and finds the part', () => {
  // Where a source opens: a spreadsheet in the sheet view at its row, any other file in the project file view.
  const pdf = { n: 1, title: 'h.pdf', site: 'page 23', project: { project: 'p-1', file: 'pf_1', kind: 'pdf', part: 9, from: 120, to: 124, at: 23, section: '' } };
  expect(partView(pdf)).toEqual({ file: { project: 'p-1', file: 'pf_1', name: 'h.pdf', kind: 'pdf', from: 120, to: 124, at: 23, place: 'page 23', section: '' } });
  const sheet = { n: 2, title: 'seats.csv', site: 'rows 61–90', project: { project: 'p-1', file: 'pf_2', kind: 'sheet', part: 3, from: 70, to: 99, at: 61, sheet: '' } };
  expect(partView(sheet)).toEqual({ sheet: { src: { from: 'project', project: 'p-1', file: 'pf_2' }, name: 'seats.csv', at: { sheet: '', row: 61 } } });
  expect(partView({ n: 3, url: 'https://example.com/' })).toBeNull();
  expect(placeLabel({ place: 'pages 12–13' })).toBe('Pages 12–13');
  expect(placeLabel({ place: '“Annual leave”', section: 'Leave › Annual leave' })).toBe('“Annual leave”');
  expect(drawnAs('markdown')).toBe('markdown');
  expect(drawnAs('code')).toBe('lines');
  expect(drawnAs('pdf')).toBe('blocks');
  // A PDF page by page, its lines kept; a deck slide by slide, its notes apart; a Word document's headings.
  const pages = blocksOf('[Page 1]\nWelcome\nto the office\n\n[Page 2, read with OCR]\nBike shed', 'pdf');
  expect(pages.map((b) => [b.kind, b.from, b.to])).toEqual([['page', 1, 1], ['para', 2, 3], ['page', 5, 5], ['para', 6, 6]]);
  expect(pages[2]).toMatchObject({ n: 2, ocr: true });
  const deck = blocksOf('Slide 1: Spring\nBeans\nSpeaker notes: Mention frost.\n\nSlide 2 (hidden): (no text)', 'powerpoint');
  expect(deck.map((b) => b.kind)).toEqual(['slide', 'para', 'note', 'slide']);
  expect(deck[3]).toMatchObject({ n: 2, text: '', hidden: true });
  const word = blocksOf('# Lockers\nKeys from reception.\nA £5 deposit.', 'word');
  expect(word.map((b) => [b.kind, b.text])).toEqual([['heading', 'Lockers'], ['para', 'Keys from reception.'], ['para', 'A £5 deposit.']]);
  expect(blocksOf('fn a() {}\n\nfn b() {}\n', 'code').map((b) => b.n)).toEqual([1, 2, 3]);
  // The part: its blocks by their lines (a Markdown block's from the renderer's data-line, 0-based and past its end).
  expect(inPart({ from: 120, to: 121 }, 121, 140)).toBe(true);
  expect(inPart({ from: 100, to: 119 }, 120, 140)).toBe(false);
  expect(blockLines('4-7')).toEqual({ from: 5, to: 7 });
  expect(blockLines('x')).toBeNull();
});

test("a big project's parts are the answer's numbered sources, each opening its file at its page, heading or row", async ({ yap, main, fakeLlm, shot }) => {
  await openView(main, 'Chat');
  const p = await yap.invoke('project_create', { name: 'Facilities', description: 'Office questions' });
  const out = await yap.invoke('project_files_add', { id: p.id, paths: [FACILITIES, HANDBOOK, SEATS] });
  expect(out.added.map((f) => f.name)).toEqual(['facilities-handbook.pdf', 'staff-handbook.md', 'seat-plan.csv']);
  expect(out.added[0].pages).toBe(40);
  await main.getByRole('button', { name: 'Projects', exact: true }).click();
  await main.getByRole('button', { name: /Facilities/ }).click();
  await page(main).getByRole('button', { name: 'New chat in Facilities' }).click();

  // The question's parts ([1], [2]) and one a search reads ([3]): the fake cites each where its words are.
  const docsOf = (record) => {
    const user = record.messages.filter((m) => m.role === 'user').at(-1);
    const text = typeof user.content === 'string' ? user.content : user.content.map((c) => c.text ?? '').join('');
    return [...text.matchAll(/<document index="(\d+)">\n<source>([^<]*)<\/source>\n<document_content>\n([\s\S]*?)\n<\/document_content>/g)].map((m) => ({ n: Number(m[1]), source: m[2], text: m[3] }));
  };
  const searched = (record) => {
    const result = record.messages.find((m) => m.role === 'tool');
    try {
      return JSON.parse(result.content).data.parts;
    } catch {
      return [...String(result.content).matchAll(/"n":(\d+),"of":\d+,"part":\d+,"source":"([^"]*)"/g)].map((m) => ({ n: Number(m[1]), source: m[2] }));
    }
  };
  let cited = null;
  fakeLlm.scriptChat([
    { tools: [{ name: 'search_project_files', args: { query: 'annual leave carry over', file: 'staff-handbook.md' } }] },
    {
      content: (record) => {
        const docs = docsOf(record);
        const shed = docs.find((d) => d.text.includes('bike shed'));
        const seat = docs.find((d) => d.text.includes('Kellbrook room'));
        // (Its text may be cut to fit the fake's 8K: found by its section.)
        const leave = searched(record).find((part) => String(part.source).endsWith('section “3. Annual leave”'));
        cited = { shed, seat, leave, docs };
        return `The bike shed is behind the north gate [${shed?.n}]. Seat 88 is in the Kellbrook room [${seat?.n}]. Full-time staff get 27 days of annual leave [${leave?.n}].`;
      },
    },
  ]);
  await box(main).fill('Where is the Kellbrook bike shed, and what is in the Kellbrook room?');
  await main.getByRole('button', { name: 'Send' }).click();
  const answer = main.locator('.cbubble.assistant').last();
  await expect(answer).toContainText('27 days of annual leave');
  expect(cited.shed, JSON.stringify(cited.docs.map((d) => d.source))).toBeTruthy();
  expect(cited.seat).toBeTruthy();
  expect(cited.leave).toBeTruthy();
  // Numbered in the files' order (the PDF's part first), the search's part after the question's.
  expect(cited.shed.n).toBe(1);
  expect(cited.seat.n).toBeGreaterThan(1);
  expect(cited.shed.source).toMatch(/^facilities-handbook\.pdf \(a PDF, 40 pages: [^)]*\): part \d+ of \d+, page 23/);
  expect(cited.leave.n).toBeGreaterThan(cited.docs.length);

  // In the answer: three pills, each naming its file and place.
  const pills = answer.locator('a.md-cite');
  await expect(pills).toHaveCount(3);
  await expect(pills.nth(0)).toHaveAttribute('title', 'facilities-handbook.pdf · page 23');
  await expect(pills.nth(2)).toHaveAttribute('title', /^staff-handbook\.md · “3\. Annual leave”$/);
  // Under it: "From the project", a chip a part.
  const row = main.locator('.msgs').getByTestId('chat-sources').last();
  await expect(row).toContainText('From the project');
  const chips = row.getByTestId('chat-project-source');
  await expect(chips).toHaveCount(3);
  await expect(chips.nth(0)).toContainText('facilities-handbook.pdf');
  await expect(chips.nth(0)).toContainText('page 23');
  await expect(chips.nth(1)).toContainText('seat-plan.csv');
  await expect(chips.nth(1)).toContainText(/rows \d+–\d+/);
  // Kept with the answer: each its place in its file.
  const conv = (await yap.invoke('chats_list')).find((c) => c.title.startsWith('Where is the Kellbrook'));
  const saved = (await yap.invoke('chat_get', { id: conv.id })).messages.at(-1).sources;
  expect(saved.map((s) => [s.n, s.title, s.project.kind])).toEqual([
    [1, 'facilities-handbook.pdf', 'pdf'],
    [cited.seat.n, 'seat-plan.csv', 'sheet'],
    [cited.leave.n, 'staff-handbook.md', 'markdown'],
  ]);
  expect(saved[0].project.at).toBe(23);
  await shot(main, '07-project-sources');

  // [1] in the answer: the PDF as Yap read it, at page 23, that page marked and in view.
  await pills.nth(0).click();
  const view = main.getByTestId('chat-project-file');
  await expect(view).toBeVisible();
  await expect(view.getByTestId('chat-project-file-name')).toHaveText('facilities-handbook.pdf');
  await expect(view.getByTestId('chat-project-file-place')).toHaveText('Page 23');
  const p23 = view.locator('[data-page="23"]');
  await expect(p23).toHaveClass(/pfmark/);
  await expect(p23).toBeInViewport();
  await expect(view.locator('.pfmark').filter({ hasText: 'Kellbrook bike shed' })).toBeInViewport();
  await expect(view.locator('[data-page="1"]')).not.toBeInViewport();
  await expect(view.getByTestId('chat-project-file-facts')).toContainText('40 pages · read only');
  await shot(main, '08-project-pdf-at-its-page');
  await theme(yap, main, 'dark');
  await shot(main, '09-project-pdf-at-its-page-dark');
  await theme(yap, main, 'light');

  // The Markdown handbook's chip: its section, as the document panel draws a document, marked.
  await chips.nth(2).click();
  await expect(view.getByTestId('chat-project-file-name')).toHaveText('staff-handbook.md');
  await expect(view.getByTestId('chat-project-file-place')).toHaveText('“3. Annual leave”');
  const marked = view.locator('.pfmd > .pfmark');
  await expect(marked.filter({ hasText: '27 days of annual leave' })).toBeInViewport();
  await expect(view.locator('.pfmd h2', { hasText: '1. Working hours' })).not.toBeInViewport();
  await shot(main, '10-project-markdown-at-its-heading');

  // The seat plan's chip: the sheet view, read only, at its part's first row.
  await chips.nth(1).click();
  const sheetView = main.getByTestId('chat-sheet');
  await expect(sheetView).toBeVisible();
  await expect(sheetView.getByTestId('chat-sheet-name')).toHaveText('seat-plan.csv');
  await expect(sheetView.getByTestId('chat-sheet-readonly')).toBeVisible();
  await expect(sheetView.getByTestId('chat-sheet-cell')).toHaveText(`A${saved[1].project.at}`);
  await expect(sheetView.getByTestId('chat-sheet-grid')).toContainText(`Seat ${saved[1].project.at}`);
  expect(saved[1].project.at).toBeGreaterThan(30);
  await shot(main, '11-project-sheet-at-its-row');
  await theme(yap, main, 'dark');
  await shot(main, '12-project-sheet-at-its-row-dark');
  await theme(yap, main, 'light');
  await yap.invoke('project_delete', { id: p.id });
});

// Asked out loud (the Voice Assistant, 16 Nov 2026): with a project's page in view, or the new chat its New chat
// started, the question starts its chat in that project, as the first typed question there does.
test("asked out loud on a project's page, or in its new chat, the chat starts in the project", async ({ yap, main, fakeLlm, shot }) => {
  const askAloud = (text) => yap.invoke('e2e_assistant_ask', { text, listenMs: 0 });
  const p = await yap.invoke('project_create', { name: 'Orchard 2027', description: 'Planting the fruit trees' });
  await yap.invoke('project_update', { id: p.id, instructions: 'Name trees by their row and number.' });
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'Projects', exact: true }).click();
  await main.getByTestId('chat-projects').getByRole('button', { name: /Orchard 2027/ }).click();
  await expect(page(main).getByRole('heading', { name: 'Orchard 2027' })).toBeVisible();

  // On the project's page: a new chat in it, the page giving way to the chat.
  fakeLlm.scriptChat([{ content: 'The plum in row 2, number 3.' }]);
  await askAloud('Which tree goes in first?');
  await expect(main.locator('.cbubble.assistant').last()).toContainText('row 2, number 3', { timeout: 20_000 });
  await expect(page(main)).toHaveCount(0);
  await expect(main.getByTestId('chat-project-line')).toContainText('In Orchard 2027');
  const sys = systemOf(askedWith(fakeLlm, 'Which tree goes in first?'));
  expect(sys).toContain('This chat is in the user\'s project "Orchard 2027". It\'s for: Planting the fruit trees');
  expect(sys).toContain('<project_instructions>\nName trees by their row and number.\n</project_instructions>');
  const first = (await yap.invoke('chats_list')).find((c) => c.title === 'Which tree goes in first?');
  await expect.poll(() => yap.readJson('projects.json').chats?.[String(first?.id)]).toBe(p.id);
  await shot(main, '04-asked-out-loud-on-the-project-page');

  // Its New chat ("New chat in Orchard 2027", nothing asked yet): the spoken question's chat is in it too.
  await main.getByTestId('chat-project-line').getByRole('button').click();
  await page(main).getByRole('button', { name: 'New chat in Orchard 2027' }).click();
  await expect(main.getByTestId('chat-project-line')).toContainText('New chat in Orchard 2027');
  fakeLlm.scriptChat([{ content: 'Water them weekly in the first summer.' }]);
  await askAloud('How often do new trees need water?');
  await expect(main.locator('.cbubble.assistant').last()).toContainText('weekly', { timeout: 20_000 });
  await expect(main.getByTestId('chat-project-line')).toContainText('In Orchard 2027');
  expect(systemOf(askedWith(fakeLlm, 'How often do new trees need water?'))).toContain('<project_instructions>');
  const second = (await yap.invoke('chats_list')).find((c) => c.title === 'How often do new trees need water?');
  await expect.poll(() => yap.readJson('projects.json').chats?.[String(second?.id)]).toBe(p.id);

  // A plain new chat (New chat in the list): in no project, as before.
  await main.getByRole('button', { name: 'New chat', exact: true }).click();
  fakeLlm.scriptChat([{ content: 'About 30 metres.' }]);
  await askAloud('How tall is a fictional lighthouse?');
  await expect(main.locator('.cbubble.assistant').last()).toContainText('30 metres', { timeout: 20_000 });
  await expect(main.getByTestId('chat-project-line')).toHaveCount(0);
  expect(systemOf(askedWith(fakeLlm, 'How tall is a fictional lighthouse?'))).not.toContain('Orchard 2027');
});
