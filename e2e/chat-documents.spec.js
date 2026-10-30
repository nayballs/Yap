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
// - Undo of a Chat edit; close, expand and Open in Notes; the split moved
//   from the keyboard and remembered; light and dark screenshots.
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
  // quiet line — never the placeholder's syntax.
  await expect(panel(main)).toBeVisible();
  await expect(panel(main).locator('.doctitle')).toHaveText('Lone wolf ranger');
  await expect(docBody(main).locator('.md-pending')).toHaveText(['Writing: Gear…', 'Writing: Skills…', 'Writing: Talents…']);
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
  expect(lastStep.system).toContain('make a document: create_document with the title and all its sections first');
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

  // Mid-turn, the person edits the text: a line of their own, and the
  // Costs section taken out. Chat's Food section lands meanwhile.
  await panel(main).getByRole('button', { name: 'Edit' }).click();
  const area = panel(main).getByRole('textbox', { name: 'Document text' });
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

  // Open in Notes: the note itself, in Chat documents.
  await panel(main).getByRole('button', { name: 'Open in Notes' }).click();
  await expect(main.locator('textarea.raw')).toHaveValue(/## Beds\n\nBeans by the fence\./);
  await expect(main.locator('.items .item').filter({ hasText: 'Garden plan' })).toBeVisible();
  const n = (await yap.invoke('notes_list')).find((x) => x.title === 'Garden plan');
  expect(n.folder).toBe('Chat documents');
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
  await yap.invoke('tts_stop');
  await setConfig(yap, main, { speakReplies: false });
});
