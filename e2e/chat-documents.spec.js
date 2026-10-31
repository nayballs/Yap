// Documents beside the chat (src-tauri/src/chat/documents.rs; src/lib/views/chat/
// DocPanel.svelte, chatDoc.svelte.js): an answer to keep is written as a
// document that opens in a panel to the right of the conversation.
// - `create_document` opens the panel on the outline at once (each section
//   still to come a quiet "Writing: Gear…" line, never the placeholder's
//   syntax); `write_section` fills one as its arguments stream, and lands;
//   the chips fold ("Wrote 3 sections in …"); a pill under the answer;
//   the note in Chat documents, every change a Chat version; the answer
//   keeps the document in chats.json, so the pill is there when the chat
//   is opened again, and reopens the panel.
// - The person's edits win: typed in the panel's Edit box while Chat writes,
//   they're kept with Chat's sections (merged in Rust); a section whose
//   placeholder they removed is refused, never brought back; the next
//   question tells the model what they changed.
// - Undo of a Chat edit, the whole turn's (never back to a half-written
//   document; none for a document the last turn made); close, expand and
//   Open in Notes; the split moved from the keyboard and remembered; light
//   and dark screenshots.
// The model is a local fake (support/fake-llm.js) scripted call by call; a
// step's `gate` holds it mid-turn while the spec edits.
import { test as base, expect, openView, expectStore, setConfig } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';

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
        name: 'chat-documents',
        config: {
          llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat' } },
          meetingReminder: 'never',
        },
      });
    },
    { scope: 'worker' },
  ],
});

const box = (main) => main.getByRole('textbox', { name: 'Message' });
const thread = (main) => main.locator('.msgs');
const panel = (main) => main.getByTestId('chat-doc');
const docBody = (main) => panel(main).locator('.docbody');
const status = (main) => main.getByTestId('chat-doc-status');
const pill = (main, title) => main.getByTestId('chat-doc-pill').filter({ hasText: title });

/** The document `create_document` made earlier in this turn (its result in the messages). */
function madeDoc(record) {
  for (const m of [...record.messages].reverse()) {
    if (m.role !== 'tool') continue;
    try {
      const r = JSON.parse(m.content);
      if (Array.isArray(r?.data?.sections)) return r.data.id;
    } catch {
      /* not JSON */
    }
  }
  return null;
}

/** A gate a scripted step waits for, and its key. */
function gate() {
  let open;
  const promise = new Promise((r) => (open = r));
  return { promise, open };
}

async function freshChat(main) {
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
}

async function ask(main, text) {
  await box(main).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
}

const noteNamed = (s, title) => s.notes.find((n) => n.title === title);

test('a document opens beside the chat, fills section by section, and its pill reopens it', async ({ yap, main, fakeLlm, shot }) => {
  const QUESTION = 'Build me a lone wolf build for a fictional ranger';
  const GEAR = '- Two rings of the fox, from the old mill\n- A short bow, kept for the last fight\n- Light boots for the swamp crossing';
  const held = gate();
  fakeLlm.scriptChat([
    {
      tools: [
        {
          name: 'create_document',
          args: {
            title: 'Lone wolf ranger',
            sections: [
              { key: 'Gear', intent: 'what to wear and where to find it' },
              { key: 'Skills', intent: 'which skills, and when' },
              { key: 'Talents', intent: 'the talents to take' },
            ],
          },
        },
      ],
    },
    { tools: [{ name: 'write_section', args: (r) => ({ note_id: madeDoc(r), key: 'Gear', markdown: GEAR }) }], gate: held.promise, stream: { gapMs: 450 } },
    {
      tools: [
        {
          name: 'write_section',
          args: (r) => ({ note_id: madeDoc(r), key: 'Skills', markdown: '| Level | Skill |\n|---|---|\n| 1 | Quick step |\n| 4 | Hide in the reeds |' }),
        },
      ],
    },
    { tools: [{ name: 'write_section', args: (r) => ({ note_id: madeDoc(r), key: 'Talents', markdown: '1. Lone Wolf\n2. Far Out Man' }) }] },
    { content: "It's open on the right." },
  ]);
  await freshChat(main);
  await ask(main, QUESTION);

  // The outline, at once: the title, and each section still to come as a
  // quiet grey line, its name and what it will say — never the
  // placeholder's syntax.
  await expect(panel(main)).toBeVisible();
  await expect(panel(main).locator('.doctitle')).toHaveText('Lone wolf ranger');
  await expect(docBody(main).locator('.md-pending')).toHaveText([
    'Gear: what to wear and where to find it',
    'Skills: which skills, and when',
    'Talents: the talents to take',
  ]);
  await expect(panel(main)).not.toContainText('yap:section');
  await shot(main, '01-outline-opens-beside-the-chat');

  // A section shows as it's written, then lands.
  held.open();
  await expect(status(main)).toHaveText(/Writing: Gear…/);
  await expect(docBody(main)).toContainText('Two rings of the fox');
  await expect(thread(main)).toContainText("It's open on the right.", { timeout: 20_000 });
  await expect(docBody(main).locator('h3')).toHaveText(['Gear', 'Skills', 'Talents']);
  await expect(docBody(main).locator('table')).toBeVisible();
  await expect(docBody(main).locator('.md-pending')).toHaveCount(0);
  await expect(status(main)).toHaveCount(0);
  await expect(thread(main).locator('.toolchip')).toHaveText([
    'Started "Lone wolf ranger" (3 sections)',
    'Wrote 3 sections in "Lone wolf ranger"',
  ]);
  await expect(pill(main, 'Lone wolf ranger')).toBeVisible();
  await shot(main, '02-written-with-its-pill');

  // The note, in Chat documents; every change a Chat version.
  await expectStore(yap, 'notes.json', (s) => {
    const n = noteNamed(s, 'Lone wolf ranger');
    return n?.folder === 'Chat documents' && n.source === 'chat' && n.content.includes('## Talents') && !n.content.includes('yap:section');
  });
  const doc = (await yap.invoke('notes_list')).find((n) => n.title === 'Lone wolf ranger');
  const versions = await yap.invoke('note_versions', { id: doc.id });
  expect(versions.filter((v) => v.field === 'notes').map((v) => v.kind)).toEqual(['chat', 'chat', 'chat', 'chat']);
  // The model was told what to do next, and the chat line is short.
  const lastStep = fakeLlm.requests.filter((r) => r.tools.includes('write_section')).at(-1);
  const results = lastStep.messages.filter((m) => m.role === 'tool').map((m) => JSON.parse(m.content));
  expect(results.at(-1).data.next).toContain('Every section is written');
  expect(lastStep.system).toContain('goes straight into a document, never the chat, never asked first');
  // A page to read: the title big, the sections' rail; a click goes there.
  await expect(docBody(main).locator('.docname')).toHaveText('Lone wolf ranger');
  const rail = main.getByTestId('chat-doc-outline');
  await rail.getByRole('button', { name: 'Sections' }).click();
  await expect(rail.getByRole('listitem')).toHaveText(['Gear', 'Skills', 'Talents']);
  await shot(main, '02b-sections-rail');
  await rail.getByRole('button', { name: 'Talents' }).click();
  await expect(rail.getByRole('list')).toHaveCount(0);
  await expect(docBody(main).locator('h3').last()).toBeInViewport();
  await expect(rail.getByRole('button', { name: 'Sections' })).toBeVisible();
  // The answer keeps it.
  await expectStore(yap, 'chats.json', (s) =>
    s.conversations.some((c) => c.messages.some((m) => m.role === 'assistant' && m.documents?.some((d) => d.id === doc.id && d.title === 'Lone wolf ranger')))
  );

  // The conversation list stepped aside for the document (Chat isn't wide
  // here): one click brings it back.
  await expect(main.getByRole('button', { name: 'New chat' })).toBeHidden();
  await main.getByRole('button', { name: 'Show your chats' }).click();
  // Another chat: the panel goes. Back: the pill is there, and reopens it.
  await main.getByRole('button', { name: 'New chat' }).click();
  await expect(panel(main)).toHaveCount(0);
  await main.locator('.conv').filter({ hasText: QUESTION.slice(0, 30) }).click();
  await expect(pill(main, 'Lone wolf ranger')).toBeVisible();
  await expect(panel(main)).toHaveCount(0);
  await pill(main, 'Lone wolf ranger').click();
  await expect(panel(main).locator('.doctitle')).toHaveText('Lone wolf ranger');
  await expect(docBody(main).locator('h3')).toHaveText(['Gear', 'Skills', 'Talents']);

  // Dark, as it looks there.
  await setConfig(yap, main, { theme: 'dark' });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe('dark');
  await shot(main, '03-dark');
  await setConfig(yap, main, { theme: 'light' });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe('light');
});

// Stream-in (2026-10-31, after Claude's own documents filling while you
// watch): a section's text flows onto the page line by line however it
// comes — here its arguments in three bursts 0.7 s apart — under a small
// caret labelled "Chat"; the sections still to write wait as grey intent
// lines; the page ends with the section exactly as saved. Motion reduced:
// shown as it comes, in its bursts.
const ROUTE = [
  '- Leave the car park by the old gate',
  '- Follow the beck up to the stone bridge',
  '- Cross it and climb the east ridge to the cairn',
  '- Lunch by the tarn, out of the wind',
  '- Down the scree path, slowly',
  '- Back along the lane to the village',
].join('\n');

/** Every frame from now on, how much of the Route section's list is on the page (its text's length). */
async function watchRoute(main) {
  await main.evaluate(() => {
    window.__flow = [];
    const tick = () => {
      const h = [...document.querySelectorAll('.docmd > h3')].find((x) => x.textContent.startsWith('Route'));
      const list = h?.nextElementSibling?.tagName === 'UL' ? h.nextElementSibling : null;
      window.__flow.push(list ? list.textContent.replace(/\s+/g, '').length : 0);
      if (window.__flow.length < 3000) requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  });
}

/** The steps the Route list grew in, frame to frame. */
async function routeSteps(main) {
  const flow = await main.evaluate(() => window.__flow);
  return flow.slice(1).map((n, i) => n - flow[i]).filter((d) => d > 0);
}

/** Script a fell walk whose Route section streams in three bursts (`gapMs` apart), held at `held`. */
function scriptWalk(fakeLlm, title, held, gapMs = 700) {
  fakeLlm.scriptChat([
    { tools: [{ name: 'create_document', args: { title, sections: [{ key: 'Route', intent: 'the way, step by step' }, { key: 'Kit', intent: 'what to carry' }] } }] },
    { tools: [{ name: 'write_section', args: (r) => ({ note_id: madeDoc(r), key: 'Route', markdown: ROUTE }) }], gate: held.promise, stream: { gapMs } },
    { tools: [{ name: 'write_section', args: (r) => ({ note_id: madeDoc(r), key: 'Kit', markdown: '- Water\n- A map' }) }] },
    { content: "It's open on the right." },
  ]);
}

test('a section flows in line by line under a labelled caret, and ends exactly as saved', async ({ yap, main, fakeLlm, shot }) => {
  const held = gate();
  scriptWalk(fakeLlm, 'Fell walk', held);
  await freshChat(main);
  await ask(main, 'Plan me a fell walk');
  // The outline: each section still to write a grey line, what it will say.
  await expect(docBody(main).locator('.md-pending')).toHaveText(['Route: the way, step by step', 'Kit: what to carry']);
  const pendingInk = await docBody(main).locator('.md-pending').first().evaluate((p) => getComputedStyle(p).color);
  const bodyInk = await docBody(main).evaluate((b) => getComputedStyle(b).color);
  expect(pendingInk).not.toBe(bodyInk);
  await expect.poll(() => fakeLlm.requests.some((r) => r.gated)).toBe(true);
  await watchRoute(main);
  held.open();

  // While it writes: a caret at the end of its text, labelled "Chat" (drawn
  // by CSS: never in the text a find or a copy sees).
  const caret = docBody(main).locator('.doccaret');
  await expect(caret).toBeVisible();
  await expect(caret).toHaveAttribute('data-who', 'Chat');
  expect(await caret.evaluate((c) => getComputedStyle(c, '::after').content)).toBe('"Chat"');
  expect(await docBody(main).locator('.docmd').evaluate((m) => m.textContent.includes('Chat'))).toBe(false);
  await expect(docBody(main).locator('.md-pending')).toHaveText(['Kit: what to carry']);
  await expect(status(main)).toHaveText(/Writing: Route…/);
  await shot(main, '01c-a-section-flowing-in');

  // It ends exactly as saved, the caret gone.
  await expect(thread(main)).toContainText("It's open on the right.", { timeout: 20_000 });
  const items = ROUTE.split('\n').map((l) => l.slice(2));
  await expect(docBody(main).locator('h3')).toHaveText(['Route', 'Kit']);
  await expect(docBody(main).locator('h3 + ul').first().locator('li')).toHaveText(items);
  await expect(caret).toHaveCount(0);
  await expect(docBody(main).locator('.md-pending')).toHaveCount(0);
  await expect(status(main)).toHaveCount(0);
  await expectStore(yap, 'notes.json', (s) => noteNamed(s, 'Fell walk')?.content.includes(`## Route\n\n${ROUTE}`));

  // Line by line, not in its bursts: many small steps, none near a burst's size.
  const steps = await routeSteps(main);
  const total = items.join('').replace(/\s+/g, '').length;
  expect(steps.reduce((a, b) => a + b, 0)).toBe(total);
  expect(steps.length).toBeGreaterThan(20);
  expect(Math.max(...steps)).toBeLessThan(total / 5);
});

test('motion reduced: a section shows as it comes, the caret still there', async ({ main, fakeLlm }) => {
  await main.emulateMedia({ reducedMotion: 'reduce' });
  try {
    await expect.poll(() => main.evaluate(() => document.documentElement.dataset.motion)).toBe('reduced');
    const held = gate();
    scriptWalk(fakeLlm, 'Still fell walk', held, 900);
    await freshChat(main);
    await ask(main, 'Plan me a still fell walk');
    await expect.poll(() => fakeLlm.requests.some((r) => r.gated)).toBe(true);
    await watchRoute(main);
    held.open();
    await expect(docBody(main).locator('.doccaret')).toBeVisible();
    await expect(thread(main)).toContainText("It's open on the right.", { timeout: 20_000 });
    await expect(docBody(main).locator('h3 + ul').first().locator('li')).toHaveCount(6);
    // In its bursts: a few steps, each a big one.
    const steps = await routeSteps(main);
    expect(steps.length).toBeLessThanOrEqual(4);
  } finally {
    await main.emulateMedia({ reducedMotion: 'no-preference' });
  }
});

// A document is a note (30 Oct 2026: Claude Haiku 5.5 made one, then said it
// couldn't write to the person's notes or a Markdown file): the model is told
// where it's kept — in the rule, create_document's result and the next
// question's context — and it is there, in Notes under Chat documents.
test('a document is a Markdown note in Notes, and the model is told so', async ({ yap, main, fakeLlm }) => {
  fakeLlm.scriptChat([
    { tools: [{ name: 'create_document', args: { title: 'Fictional camping kit', sections: [{ key: 'Clothes', intent: 'what to wear' }] } }] },
    { tools: [{ name: 'write_section', args: (r) => ({ note_id: madeDoc(r), key: 'Clothes', markdown: '- A fleece\n- Spare socks' }) }] },
    { content: "It's open on the right, and saved in your Notes under Chat documents." },
  ]);
  await freshChat(main);
  await ask(main, 'Make me a camping kit list and put it in my notes as a Markdown file');
  await expect(thread(main)).toContainText('saved in your Notes under Chat documents', { timeout: 20_000 });
  const steps = fakeLlm.requests.filter((r) => r.user.includes('camping kit list'));
  // The rule, in the system prompt (the same every turn).
  expect(steps[0].system).toContain('A document is a Markdown note, saved in Notes under Chat documents');
  // create_document's result says where it's kept, and the last section's
  // the line for someone who asked for it in their notes.
  const results = steps.at(-1).messages.filter((m) => m.role === 'tool').map((m) => JSON.parse(m.content));
  expect(results.find((r) => Array.isArray(r?.data?.sections)).data.saved).toBe(
    'Saved as a Markdown note in Notes, in the Chat documents folder: the user can open it there, or export it as a .md file (Export as Markdown).'
  );
  expect(results.at(-1).data.next).toContain("\"It's open on the right, and saved in your Notes under Chat documents.\"");
  // It is there.
  await expectStore(yap, 'notes.json', (s) => noteNamed(s, 'Fictional camping kit')?.folder === 'Chat documents');
  // The next question about it: the chat's documents, saved in Notes as Markdown.
  fakeLlm.scriptChat([{ content: 'Yes: it is in Notes, under Chat documents.' }]);
  await ask(main, 'Is the fictional kit list saved in my notes?');
  await expect(thread(main)).toContainText('Yes: it is in Notes, under Chat documents.');
  const next = fakeLlm.requests.find((r) => r.user.includes('Is the fictional kit list saved in my notes?'));
  expect(next.user).toContain('Documents in this chat, open beside it and saved in Notes (Chat documents) as Markdown notes');
});

test("the person's edits win: typed while Chat writes they stay, a section they removed isn't brought back", async ({ yap, main, fakeLlm, shot }) => {
  // Food is held until the person has typed (it lands while their typing
  // waits to be saved, or just after); Costs until their edit is saved.
  const held = gate();
  const costs = gate();
  fakeLlm.scriptChat([
    {
      tools: [
        {
          name: 'create_document',
          args: {
            title: 'Weekend in a fictional town',
            sections: [
              { key: 'Days', intent: 'day by day' },
              { key: 'Food', intent: 'where to eat' },
              { key: 'Costs', intent: 'what it costs' },
            ],
          },
        },
      ],
    },
    { tools: [{ name: 'write_section', args: (r) => ({ note_id: madeDoc(r), key: 'Days', markdown: 'Saturday: the walls. Sunday: the museum.' }) }] },
    { tools: [{ name: 'write_section', args: (r) => ({ note_id: madeDoc(r), key: 'Food', markdown: 'Tea rooms on the high street.' }) }], gate: held.promise },
    { tools: [{ name: 'write_section', args: (r) => ({ note_id: madeDoc(r), key: 'Costs', markdown: 'About 200 pounds.' }) }], gate: costs.promise },
    { content: "It's open on the right." },
  ]);
  await freshChat(main);
  await ask(main, 'Plan me a weekend away');
  await expect(docBody(main)).toContainText('Saturday: the walls.');
  await expect.poll(() => fakeLlm.requests.some((r) => r.gated)).toBe(true);

  // Mid-turn, the person edits the text (Source: its markdown): a line of
  // their own, and the Costs section taken out. Chat's Food section lands
  // meanwhile.
  await panel(main).getByRole('button', { name: 'Source' }).click();
  const area = panel(main).getByRole('textbox', { name: 'Document source (Markdown)' });
  await expect(area).toBeFocused();
  const now = await area.inputValue();
  expect(now).toContain('<!-- yap:section Costs');
  await area.fill(`${now.replace(/<!-- yap:section Costs[^\n]*-->\n?/, '').trimEnd()}\n\nBring an umbrella.\n`);
  held.open();

  // Both kept: their words and Chat's section.
  await expectStore(yap, 'notes.json', (s) => {
    const c = noteNamed(s, 'Weekend in a fictional town')?.content ?? '';
    return c.includes('Bring an umbrella.') && c.includes('## Food\n\nTea rooms on the high street.') && !c.includes('Costs');
  });
  // Then Chat tries the section they took out: refused, not brought back.
  costs.open();
  await expect(thread(main)).toContainText("It's open on the right.", { timeout: 20_000 });
  await expectStore(yap, 'notes.json', (s) => !(noteNamed(s, 'Weekend in a fictional town')?.content ?? 'Costs').includes('Costs'));
  await expect(area).toHaveValue(/## Food\n\nTea rooms on the high street\./);
  await expect(area).toHaveValue(/Bring an umbrella\./);
  await expect(thread(main).locator('.toolchip')).toContainText([`Didn't write "Costs": it's no longer in "Weekend in a fictional town"`]);
  const refused = fakeLlm.requests.filter((r) => r.tools.includes('write_section')).at(-1).messages.filter((m) => m.role === 'tool').map((m) => JSON.parse(m.content)).at(-1);
  expect(refused.success).toBe(false);
  expect(refused.error).toContain("Don't write it again or add it back");
  await shot(main, '04-their-edit-kept');

  // The next question tells the model what they changed, once.
  fakeLlm.scriptChat([{ content: 'Noted: an umbrella, and no costs.' }]);
  await ask(main, 'Anything else I should pack?');
  await expect(thread(main)).toContainText('Noted: an umbrella, and no costs.');
  const told = fakeLlm.requests.at(-1).user;
  expect(told).toContain('Documents in this chat');
  expect(told).toContain('Since you last saw "Weekend in a fictional town", the user changed it');
  expect(told).toContain('+ put in: "Bring an umbrella."');
  expect(told).toContain('- took out: "<!-- yap:section Costs');
});

test("Undo takes back Chat's change to a document", async ({ yap, main, fakeLlm }) => {
  fakeLlm.scriptChat([
    { tools: [{ name: 'create_document', args: { title: 'Packing list', sections: [{ key: 'Clothes', intent: 'what to wear' }] } }] },
    { tools: [{ name: 'write_section', args: (r) => ({ note_id: madeDoc(r), key: 'Clothes', markdown: '- A warm coat' }) }] },
    { content: "It's open on the right." },
  ]);
  await freshChat(main);
  await ask(main, 'Make me a packing list');
  await expect(docBody(main)).toContainText('A warm coat');
  await expect(thread(main)).toContainText("It's open on the right.");
  const doc = (await yap.invoke('notes_list')).find((n) => n.title === 'Packing list');

  // A later question changes it (edit_note): the panel follows.
  fakeLlm.scriptChat([
    { tools: [{ name: 'edit_note', args: { note_id: doc.id, field: 'content', old_text: '- A warm coat', new_text: '- A warm coat\n- Wellies' } }] },
    { content: 'Added wellies.' },
  ]);
  await ask(main, 'Add wellies');
  await expect(docBody(main)).toContainText('Wellies');
  await expect(pill(main, 'Packing list')).toHaveCount(2);

  // Undo: back as it was, and it's a version of its own.
  await panel(main).getByRole('button', { name: "Undo Chat's last change" }).click();
  await expect(docBody(main)).not.toContainText('Wellies');
  await expectStore(yap, 'notes.json', (s) => !noteNamed(s, 'Packing list')?.content.includes('Wellies'));
  const kinds = (await yap.invoke('note_versions', { id: doc.id })).filter((v) => v.field === 'notes').map((v) => v.kind);
  expect(kinds[0]).toBe('restored');
  await expect(panel(main).getByRole('button', { name: "Undo Chat's last change" })).toHaveCount(0);
});

// Undo is Chat's whole last turn (notes/versions.rs `in_turn`). A turn
// writes a document a call at a time — its outline, then each section, each
// a version — and Undo used to go back one call: after "Wrote 5 sections"
// it brought the last section's placeholder back ("1 section not written
// yet", the maintainer's document, 31 Oct 2026).
test("Undo takes back Chat's whole turn, never half a document", async ({ yap, main, fakeLlm }) => {
  const undo = () => panel(main).getByRole('button', { name: "Undo Chat's last change" });
  fakeLlm.scriptChat([
    {
      tools: [
        {
          name: 'create_document',
          args: { title: 'Mage build', sections: [{ key: 'Gear', intent: 'what to wear' }, { key: 'Levels', intent: 'level by level' }, { key: 'Tips', intent: 'tips' }] },
        },
      ],
    },
    { tools: [{ name: 'write_section', args: (r) => ({ note_id: madeDoc(r), key: 'Gear', markdown: '- A frost staff' }) }] },
    { tools: [{ name: 'write_section', args: (r) => ({ note_id: madeDoc(r), key: 'Levels', markdown: '| Level | Pick |\n|---|---|\n| 20 | Frost nova |' }) }] },
    { tools: [{ name: 'write_section', args: (r) => ({ note_id: madeDoc(r), key: 'Tips', markdown: '- Keep your distance' }) }] },
    { content: "It's open on the right." },
  ]);
  await freshChat(main);
  await ask(main, 'Make me a mage build');
  await expect(thread(main)).toContainText("It's open on the right.", { timeout: 20_000 });
  await expect(docBody(main)).toContainText('Keep your distance');
  // The chips count what was written.
  await expect(thread(main).locator('.toolchip').filter({ hasText: 'Mage build' })).toHaveText(['Started "Mage build" (3 sections)', 'Wrote 3 sections in "Mage build"']);
  const doc = (await yap.invoke('notes_list')).find((n) => n.title === 'Mage build');
  // Its outline and three sections: one turn's versions.
  const made = (await yap.invoke('note_versions', { id: doc.id })).filter((v) => v.field === 'notes');
  expect(made.map((v) => v.kind)).toEqual(['chat', 'chat', 'chat', 'chat']);
  expect(made[0].turn).toBeGreaterThan(0);
  expect(new Set(made.map((v) => v.turn)).size).toBe(1);
  // That turn made the document: nothing of it before, so no Undo — never
  // back to a half-written one.
  await expect(undo()).toHaveCount(0);
  await expect(status(main)).toHaveCount(0);

  // A later turn rewrites two sections: one change, undone whole.
  fakeLlm.scriptChat([
    { tools: [{ name: 'rewrite_section', args: { note_id: doc.id, heading: 'Gear', markdown: '- A fire staff' } }] },
    { tools: [{ name: 'rewrite_section', args: { note_id: doc.id, heading: 'Tips', markdown: '- Stand in the back row' } }] },
    { content: 'Switched to fire.' },
  ]);
  await ask(main, 'Make it a fire mage instead');
  await expect(thread(main)).toContainText('Switched to fire.', { timeout: 20_000 });
  await expect(docBody(main)).toContainText('Stand in the back row');
  const changed = (await yap.invoke('note_versions', { id: doc.id })).filter((v) => v.field === 'notes');
  expect(changed[0].turn).toBe(changed[1].turn);
  expect(changed[1].turn).not.toBe(made[0].turn);
  await undo().click();
  // Back as the first turn left it: every section written.
  await expect(docBody(main)).toContainText('A frost staff');
  await expect(docBody(main)).toContainText('Keep your distance');
  await expect(docBody(main)).not.toContainText('A fire staff');
  await expect(docBody(main)).not.toContainText('Stand in the back row');
  await expect(status(main)).toHaveCount(0);
  await expectStore(yap, 'notes.json', (s) => {
    const c = noteNamed(s, 'Mage build')?.content ?? '';
    return c.includes('- A frost staff') && c.includes('- Keep your distance') && !c.includes('yap:section') && !c.includes('fire staff');
  });
});

test('the panel closes, expands, moves its split from the keyboard and opens in Notes', async ({ yap, main, fakeLlm, shot }) => {
  fakeLlm.scriptChat([
    { tools: [{ name: 'create_document', args: { title: 'Garden plan', sections: [{ key: 'Beds', intent: 'what goes where' }] } }] },
    { tools: [{ name: 'write_section', args: (r) => ({ note_id: madeDoc(r), key: 'Beds', markdown: 'Beans by the fence.' }) }] },
    { content: "It's open on the right." },
  ]);
  await freshChat(main);
  await ask(main, 'Plan my vegetable garden');
  await expect(thread(main)).toContainText("It's open on the right.");
  await expect(docBody(main)).toContainText('Beans by the fence.');

  // Expand: it covers the conversation; Esc (or the button) puts it back.
  await panel(main).getByRole('button', { name: 'Expand to full width' }).click();
  await expect(main.locator('.docslot.cover')).toHaveCount(1);
  await shot(main, '05-expanded');
  await main.keyboard.press('Escape');
  await expect(main.locator('.docslot.cover')).toHaveCount(0);
  await panel(main).getByRole('button', { name: 'Expand to full width' }).click();
  await panel(main).getByRole('button', { name: 'Back beside the chat' }).click();
  await expect(main.locator('.docslot.cover')).toHaveCount(0);

  // The split, from the keyboard: narrower, and remembered when it opens again.
  const split = main.getByRole('separator', { name: 'Resize the document' });
  const before = Number(await split.getAttribute('aria-valuenow'));
  await split.focus();
  await main.keyboard.press('ArrowRight');
  await expect(split).toHaveAttribute('aria-valuenow', String(before - 24));
  const width = await main.locator('.docslot').evaluate((el) => el.getBoundingClientRect().width);
  expect(Math.round(width)).toBe(before - 24);

  // ✕, then the pill again: the same width.
  await panel(main).getByRole('button', { name: 'Close the document' }).click();
  await expect(panel(main)).toHaveCount(0);
  await pill(main, 'Garden plan').click();
  await expect(panel(main)).toBeVisible();
  await expect(split).toHaveAttribute('aria-valuenow', String(before - 24));

  // Open in Notes (in ⋯): the note itself, in Chat documents.
  await panel(main).getByRole('button', { name: 'More for the document' }).click();
  await main.getByRole('menuitem', { name: 'Open in Notes' }).click();
  await expect(main.locator('textarea.raw')).toHaveValue(/## Beds\n\nBeans by the fence\./);
  await expect(main.locator('.items .item').filter({ hasText: 'Garden plan' })).toBeVisible();
  const n = (await yap.invoke('notes_list')).find((x) => x.title === 'Garden plan');
  expect(n.folder).toBe('Chat documents');
});

// A change to the open document reworks it (documents.rs `rewrite_section`):
// a section rewritten whole as it streams, a new one added at the end, the
// model told what the question means for it (`steer`, a model on this PC);
// the summary sits under the title.
test('a change reworks the open document: a section rewritten whole, one added', async ({ yap, main, fakeLlm, shot }) => {
  fakeLlm.scriptChat([
    {
      tools: [
        {
          name: 'create_document',
          args: { title: 'Fictional cipher', summary: 'A careful, slow build for a fictional cipher.', sections: [{ key: 'Powers', intent: 'which' }, { key: 'Gear', intent: 'what' }] },
        },
      ],
    },
    { tools: [{ name: 'write_section', args: (r) => ({ note_id: madeDoc(r), key: 'Powers', markdown: '- Mind wave\n- Eyestrike' }) }] },
    { tools: [{ name: 'write_section', args: (r) => ({ note_id: madeDoc(r), key: 'Gear', markdown: '- **Early:** a staff\n- **Late:** a pistol' }) }] },
    { content: "It's open on the right." },
  ]);
  await freshChat(main);
  await ask(main, 'Make me a build for a fictional cipher');
  await expect(thread(main)).toContainText("It's open on the right.", { timeout: 20_000 });
  await expect(docBody(main).locator('.docmd > p').first()).toHaveText('A careful, slow build for a fictional cipher.');
  const doc = (await yap.invoke('notes_list')).find((n) => n.title === 'Fictional cipher');

  const held = gate();
  fakeLlm.scriptChat([
    {
      tools: [{ name: 'rewrite_section', args: { note_id: doc.id, heading: 'Powers', markdown: '| Level | Power |\n|---|---|\n| 1 | Mind wave |\n| 3 | Eyestrike |' } }],
      gate: held.promise,
      stream: { gapMs: 300 },
    },
    { tools: [{ name: 'rewrite_section', args: { note_id: doc.id, heading: 'Level by level', markdown: '| Level | Pick |\n|---|---|\n| 1 | Mind wave |' } }] },
    { content: 'Done: the powers go level by level now.' },
  ]);
  await ask(main, 'Can you change it so the powers go level by level?');
  held.open();
  await expect(status(main)).toHaveText(/Writing: Powers…/);
  await expect(thread(main)).toContainText('Done: the powers go level by level now.', { timeout: 20_000 });
  await expect(docBody(main).locator('h3')).toHaveText(['Powers', 'Gear', 'Level by level']);
  await expect(docBody(main).locator('table')).toHaveCount(2);
  await expect(docBody(main)).not.toContainText('- Mind wave');
  await expect(thread(main).locator('.toolchip').filter({ hasText: 'Fictional cipher' })).toHaveText([
    'Started "Fictional cipher" (2 sections)',
    'Wrote 2 sections in "Fictional cipher"',
    'Rewrote "Powers" in "Fictional cipher"',
    'Added "Level by level" to "Fictional cipher"',
  ]);
  await shot(main, '06-a-section-rewritten-and-one-added');
  // The model was told: this changes the open document.
  const step = fakeLlm.requests.find((r) => r.user.includes('powers go level by level'));
  expect(step.user).toContain('This changes "Fictional cipher"');
  expect(step.tools).toContain('rewrite_section');
  const versions = await yap.invoke('note_versions', { id: doc.id });
  expect(versions.filter((v) => v.field === 'notes').map((v) => v.kind).slice(0, 2)).toEqual(['chat', 'chat']);
});

// The Voice Assistant (chat/assistant.rs): a document only when one is asked
// for out loud. Its question carries the spoken note; the document opens in
// Chat, shown here, as it fills; the voice says one line. (Yap never brings
// Chat up over another app for it: the assistant never takes the focus.)
test('asked out loud for a plan: the document opens in Chat as it fills, and the voice says a line', async ({ yap, main, fakeLlm }) => {
  await setConfig(yap, main, { speakReplies: true });
  await freshChat(main);
  await yap.invoke('tts_e2e_spoken', { clear: true });
  fakeLlm.scriptChat([
    { tools: [{ name: 'create_document', args: { title: 'Hike plan', sections: [{ key: 'Route', intent: 'the way' }, { key: 'Kit', intent: 'what to bring' }] } }] },
    { tools: [{ name: 'write_section', args: (r) => ({ note_id: madeDoc(r), key: 'Route', markdown: 'Up the east ridge, down by the lake.' }) }] },
    { tools: [{ name: 'write_section', args: (r) => ({ note_id: madeDoc(r), key: 'Kit', markdown: '- Water\n- A map' }) }] },
    { content: "I've put the plan in Chat." },
  ]);
  await yap.invoke('e2e_assistant_ask', { text: 'make me a plan for a hike up a fictional hill', listenMs: 0 });
  await expect(panel(main).locator('.doctitle')).toHaveText('Hike plan', { timeout: 15_000 });
  await expect(docBody(main)).toContainText('Up the east ridge');
  await expect(pill(main, 'Hike plan')).toBeVisible();
  await expect
    .poll(async () => (await yap.invoke('tts_e2e_spoken')).map((s) => s.text).join(' '), { timeout: 15_000 })
    .toContain("I've put the plan in Chat.");
  const asked = fakeLlm.requests.find((r) => r.tools.includes('create_document') && r.user.includes('fictional hill'));
  expect(asked.user).toContain('This was asked out loud: answer in speech. Make a document only when they ask for one');
  // Asked with Chat in front (Yap's window has the focus): the line points
  // at the panel, never "it's in Chat" to someone reading Chat.
  expect(asked.user).toContain("say in one short sentence that it's open on the right.");
  expect(asked.user).not.toContain("that it's in Chat");
  await yap.invoke('tts_stop');
  // From another app (Yap's window not in front): "it's in Chat".
  await yap.invoke('e2e_focus', { focused: false });
  fakeLlm.scriptChat([{ content: 'You are welcome.' }]);
  await yap.invoke('e2e_assistant_ask', { text: 'thanks for the fictional hike plan', listenMs: 0 });
  await expect.poll(() => fakeLlm.requests.some((r) => r.user.includes('thanks for the fictional hike plan')), { timeout: 15_000 }).toBe(true);
  const away = fakeLlm.requests.find((r) => r.user.includes('thanks for the fictional hike plan'));
  expect(away.user).toContain("say in one short sentence that it's in Chat.");
  await expect.poll(async () => (await yap.invoke('tts_e2e_spoken')).map((s) => s.text).join(' '), { timeout: 15_000 }).toContain('You are welcome.');
  await yap.invoke('tts_stop');
  await yap.invoke('e2e_focus', { focused: true });
  await setConfig(yap, main, { speakReplies: false });
});
