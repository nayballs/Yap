// Documents beside the chat for a chat on your Claude plan (desktop/src-tauri/src/integrations/
// claude_live/documents.rs, chat/turn/plan.rs; 16 Nov 2026): the plan's own Claude Code calls Yap's
// create_document, write_section, read_document and edit_document over Yap's run endpoint, and each
// call runs the tool loop's own code and rules (chat/documents.rs, chat/cards/outline.rs) for the chat
// turn it belongs to. Before, a plan chat made no documents, so the outline card never showed there.
//
// Claude Code is this build's stand-in (`yap.exe fake-claude <script>`, ai/claude_code/fake.rs),
// switched per test with the debug-only `e2e_claude_script`: the plan chat's session streams its tool
// calls as Claude Code does, then `_mcp` calls Yap's endpoint as Claude Code would (`{{mcp_id}}`: the
// note id the last call's answer gave), hands the answer back as its tool result and, with `dump`,
// writes what it got to a file this spec reads. Nothing of the person's Claude Code or the internet.
// Fictional words only.
//
// - A new document: its outline is a card in the thread first and nothing is written (the call is
//   held) until Go; then the document opens beside the chat, fills section by section, the chips fold,
//   and the answer keeps its pill. Light and dark.
// - An edit: "Make the introduction shorter" changes that block (edit_document), as a version of Chat's.
// - The person's words win: typed into the page, they stay when Claude edits again — its stale edit of
//   their block is refused, and the next question tells it what they changed.
// - Edit on the card: the writing follows the person's sections.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, expectStore, openView, setConfig, jsonLines } from './support/fixtures.js';
import { fakeClaude, planConfig, SCRIPTS as PLAN_SCRIPTS } from './support/claude-plan.js';
import { RUNS } from './support/yap.js';

const HERE = path.join(RUNS, 'plan-documents-spec');
const SCRIPTS = path.join(HERE, 'scripts');
const SID = '{{session_id}}';
const MODEL = 'claude-haiku-5-5';
const DOC = '{{mcp_id}}';

// ---- the stand-in's script ----

const ev = (event) => ({ type: 'stream_event', event, parent_tool_use_id: null, session_id: SID });
const CHAT_INIT = {
  type: 'system',
  subtype: 'init',
  cwd: '{{cwd}}',
  session_id: SID,
  tools: ['mcp__yap__create_document', 'mcp__yap__write_section', 'mcp__yap__read_document', 'mcp__yap__edit_document'],
  mcp_servers: [{ name: 'yap', status: 'connected' }],
  model: MODEL,
  permissionMode: 'dontAsk',
  apiKeySource: 'none',
  claude_code_version: '2.1.296',
  capabilities: ['interrupt_receipt_v1'],
};
const whole = (id, content, stop) => ({
  type: 'assistant',
  message: { id, type: 'message', role: 'assistant', model: MODEL, content, stop_reason: stop },
  parent_tool_use_id: null,
  session_id: SID,
});
const result = (text) => ({
  type: 'result',
  subtype: 'success',
  is_error: false,
  duration_ms: 900,
  num_turns: 2,
  result: text,
  session_id: SID,
  total_cost_usd: 0.003,
  usage: { input_tokens: 5200, output_tokens: 30, cache_creation_input_tokens: 0, cache_read_input_tokens: 0 },
  permission_denials: [],
  terminal_reason: 'completed',
});
/** The model calls `name` in message `msg`, streamed (its start first) then whole, as Claude Code writes it. */
const calls = (msg, id, name, input) => [
  ev({ type: 'message_start', message: { id: msg, type: 'message', role: 'assistant', model: MODEL, content: [] } }),
  ev({ type: 'content_block_start', index: 0, content_block: { type: 'tool_use', id, name, input: {} } }),
  ev({ type: 'content_block_delta', index: 0, delta: { type: 'input_json_delta', partial_json: JSON.stringify(input) } }),
  ev({ type: 'content_block_stop', index: 0 }),
  whole(msg, [{ type: 'tool_use', id, name, input }], 'tool_use'),
  ev({ type: 'message_stop' }),
];
/** …one of Yap's: its call, Yap's endpoint called with it (what it gave written to `dump`), the answer its tool result. */
const yapCall = (msg, id, tool, input, dump) => [...calls(msg, id, `mcp__yap__${tool}`, input), { _mcp: { tool, arguments: input, result_for: id, dump } }];
/** The answer, streamed then whole, and the turn's result. */
const answers = (msg, text) => [
  ev({ type: 'message_start', message: { id: msg, type: 'message', role: 'assistant', model: MODEL, content: [] } }),
  ev({ type: 'content_block_start', index: 0, content_block: { type: 'text', text: '' } }),
  ev({ type: 'content_block_delta', index: 0, delta: { type: 'text_delta', text } }),
  ev({ type: 'content_block_stop', index: 0 }),
  whole(msg, [{ type: 'text', text }], 'end_turn'),
  ev({ type: 'message_stop' }),
  result(text),
];
/** One plan chat turn: the question (its words written to `args`, when given), then `lines`. */
const turn = (args, ...lines) => [{ _wait_user: true }, ...(args ? [{ _dump_args: args }] : []), CHAT_INIT, ...lines.flat(Infinity)];

/** A script of plan chat turns. */
function script(name, chatTurns) {
  fs.mkdirSync(SCRIPTS, { recursive: true });
  const file = path.join(SCRIPTS, `${name}.jsonl`);
  const lines = [{ _for: 'chat' }, ...chatTurns.flat(Infinity), { _wait_user: true }];
  fs.writeFileSync(file, lines.map((l) => JSON.stringify(l)).join('\n') + '\n');
  return file;
}

/** A fresh file for the stand-in to write its MCP calls (or arguments) to. */
function fresh(name) {
  fs.mkdirSync(HERE, { recursive: true });
  const file = path.join(HERE, name);
  fs.rmSync(file, { force: true });
  return file;
}
const linesOf = (file) => jsonLines(file);
const resultOf = (call) => JSON.parse(call.content[0].text);

// ---- the document ----

const TITLE = 'Composting at home';
const OUTLINE = {
  title: TITLE,
  summary: 'A one-page guide to making compost in a small garden.',
  sections: [
    { key: 'Introduction', intent: 'why compost, and what it takes' },
    { key: 'What goes in', intent: 'greens and browns' },
    { key: 'Troubleshooting', intent: 'smells, pests and a slow heap' },
  ],
};
const WORDS = {
  Introduction: 'Composting turns kitchen scraps and garden waste into rich, crumbly soil. It takes a bin or a corner of the garden, a mix of materials and a few months of patience.',
  'What goes in': 'Mix greens (vegetable peelings, grass cuttings, coffee grounds) with browns (dry leaves, cardboard, twigs).',
  Troubleshooting: '- **Smells:** add more browns.\n- **Flies:** bury fresh scraps.\n- **Slow heap:** turn it and keep it damp.',
};
const SHORT_INTRO = 'Composting turns scraps into rich soil in a few months.';
const TYPED = ' Crushed eggshells are fine too.';
const TIP = '- **Turning:** turn the heap every two weeks.';

/** The note's text once written as outlined (`documents::skeleton`, each section filled). */
const written = (o = OUTLINE) => `# ${o.title}\n\n${o.summary}\n\n${o.sections.map((s) => `## ${s.key}\n\n${WORDS[s.key]}\n`).join('\n')}`;

/**
 * A block's id and hash as Yap's read_document gives them (chat/documents/blocks.rs `identify`: FNV-1a 64
 * of its kind's letter and its text; the id the letter and the low 5 hex, the hash the high 8).
 */
function block(kind, text) {
  let h = 0xcbf29ce484222325n;
  for (const b of Buffer.from(`${kind}\n${text}`, 'utf8')) {
    h ^= BigInt(b);
    h = (h * 0x100000001b3n) & 0xffffffffffffffffn;
  }
  return { id: kind + (h & 0xfffffn).toString(16).padStart(5, '0'), hash: (h >> 32n).toString(16).padStart(8, '0') };
}

/** The first turn: the outline, then each section, then the answer (`dump`: each call's answer). */
const makes = (dump, args, outline = OUTLINE) =>
  turn(
    args,
    yapCall('m1', 'toolu_doc', 'create_document', outline, dump),
    // (What the card's Go left: the sections as outlined.)
    outline.sections.map((s, i) => yapCall(`m${i + 2}`, `toolu_s${i}`, 'write_section', { note_id: DOC, key: s.key, markdown: WORDS[s.key] }, dump)),
    answers('m9', "It's open on the right.")
  );

// ---- the app ----

const test = base.extend({
  yapOptions: [
    async ({}, use) => {
      await use({
        name: 'plan-documents',
        env: fakeClaude(path.join(PLAN_SCRIPTS, 'plan-chat.jsonl')),
        config: planConfig({ 'claude-plan': 'claude-plan' }, { memoryLearnChats: false, webSearch: false, chatOutlineReview: true }),
      });
    },
    { scope: 'worker' },
  ],
});

const thread = (main) => main.locator('.msgs');
const lastAnswer = (main) => thread(main).locator('.cbubble.assistant').last();
const chipsOfLast = (main) => thread(main).locator('.toolchips').last().locator('.toolchip');
const panel = (main) => main.getByTestId('chat-doc');
const docBody = (main) => panel(main).locator('.docbody');
const page = (main) => panel(main).locator('.docedit');
const card = (main) => thread(main).getByTestId('chat-outline-card');
const pill = (main, title) => main.getByTestId('chat-doc-pill').filter({ hasText: title });
const play = (yap, name, chatTurns) => yap.invoke('e2e_claude_script', { script: script(name, chatTurns) });
/** The newest note called `title` (its id the highest: each test makes its own). */
const noteNamed = (s, title) =>
  (s?.notes ?? []).filter((n) => n.title === title).reduce((a, n) => (!a || n.id > a.id ? n : a), undefined);

async function send(main, text) {
  await main.getByRole('textbox', { name: 'Message' }).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
}

async function freshChat(main) {
  await openView(main, 'Chat');
  // (A document left open by an earlier test: the panel closed, so the chat list shows.)
  const closePanel = panel(main).getByRole('button', { name: 'Close the panel' });
  if (await closePanel.count()) await closePanel.click();
  const showChats = main.getByRole('button', { name: 'Show your chats' });
  if (await showChats.isVisible()) await showChats.click();
  await main.getByRole('button', { name: 'New chat' }).first().click();
}

async function theme(yap, main, which) {
  await setConfig(yap, main, { theme: which });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe(which);
}

/** The newest note called `title`, once its text is `want`. */
async function noteIs(yap, title, want) {
  let now = null;
  try {
    await expectStore(yap, 'notes.json', (s) => (now = noteNamed(s, title)?.content) === want, `the note to read as ${JSON.stringify(want).slice(0, 80)}…`);
  } catch (e) {
    expect(now, 'the note as saved').toBe(want);
    throw e;
  }
}

test.beforeEach(async ({ yap, main }) => {
  await setConfig(yap, main, { agentsEnabled: false, webSearch: false, chatOutlineReview: true, theme: 'light' });
});

test('a plan chat writes a document: the outline first, nothing written until Go; then it opens beside the chat and fills, and the answer keeps its pill', async ({ yap, main, shot }) => {
  const DUMP = fresh('make.jsonl');
  const LIST = fresh('make-list.jsonl');
  await play(yap, 'make', [
    // (What the session was offered: Yap's endpoint's tools/list.)
    [{ _wait_user: true }, { _mcp: { list: true, dump: LIST } }, ...makes(DUMP, null).slice(1)],
  ]);
  await freshChat(main);
  const before = noteNamed(yap.readJson('notes.json'), TITLE)?.id;
  await send(main, 'Write me a one-page guide to composting at home as a document');

  // The card first: the title, each heading and its line, Go and Edit — and nothing written.
  await expect(card(main)).toHaveAttribute('data-state', 'asking', { timeout: 20_000 });
  await expect(card(main).locator('.otitle')).toContainText(`The outline · ${TITLE}`);
  await expect(card(main).locator('.osum')).toHaveText(OUTLINE.summary);
  await expect(card(main).locator('.okey')).toHaveText(OUTLINE.sections.map((s) => s.key));
  await expect(card(main).locator('.ointent')).toHaveText(OUTLINE.sections.map((s) => s.intent));
  await expect(main.getByTestId('chat-activity')).toContainText('Waiting for you: the outline');
  await expect(panel(main)).toHaveCount(0);
  expect(noteNamed(yap.readJson('notes.json'), TITLE)?.id).toBe(before);
  // (The call is held: the stand-in hasn't had its answer.)
  expect(linesOf(DUMP)).toEqual([]);
  await shot(main, '01-plan-outline-card-light');
  await theme(yap, main, 'dark');
  await shot(main, '02-plan-outline-card-dark');
  await theme(yap, main, 'light');

  // Go: made as outlined, the panel opens on it and fills; the chips fold; the card says how it ended.
  await card(main).getByRole('button', { name: 'Go', exact: true }).click();
  await expect(lastAnswer(main)).toHaveText("It's open on the right.", { timeout: 30_000 });
  await expect(docBody(main).locator('h3')).toHaveText(OUTLINE.sections.map((s) => s.key));
  await expect(docBody(main)).toContainText(WORDS.Introduction);
  await expect(card(main)).toHaveAttribute('data-state', 'allowed');
  await expect(card(main)).toContainText('Written as outlined');
  await expect(chipsOfLast(main)).toHaveText([`Started "${TITLE}" (3 sections)`, `Wrote 3 sections in "${TITLE}"`]);
  await expect(pill(main, TITLE)).toBeVisible();
  await noteIs(yap, TITLE, written());
  // What Claude Code read: the outline answered, then each section written.
  const [made, ...wrote] = linesOf(DUMP);
  expect(resultOf(made)).toMatchObject({ success: true, data: { title: TITLE, sections: OUTLINE.sections.map((s) => s.key) } });
  expect(resultOf(made).data.outline).toContain('The user read the outline and said go');
  expect(wrote.map((w) => resultOf(w).success)).toEqual([true, true, true]);
  expect(resultOf(wrote.at(-1)).data.next).toContain('Every section is written');
  // The session lists the rich set: two read, four write.
  const listed = linesOf(LIST)[0];
  const tools = Object.fromEntries((listed?.tools ?? []).map((t) => [t.name, t.readOnlyHint]));
  expect(tools).toMatchObject({ create_document: false, write_section: false, read_document: true, edit_document: false, document_history: true, restore_document: false });
  // Kept with the answer (its pill, the chat's document from now on); every change a version of Chat's.
  const id = noteNamed(yap.readJson('notes.json'), TITLE).id;
  await expectStore(yap, 'chats.json', (s) => s.conversations.some((c) => c.messages.some((m) => m.role === 'assistant' && m.documents?.some((d) => d.id === id && d.title === TITLE))));
  const versions = (await yap.invoke('note_versions', { id })).filter((v) => v.field === 'notes');
  expect(versions.length).toBe(4);
  expect(versions.every((v) => v.kind === 'chat')).toBe(true);
  await shot(main, '03-plan-document-written-light');
  await theme(yap, main, 'dark');
  await shot(main, '04-plan-document-written-dark');
  await theme(yap, main, 'light');
});

test("a plan chat edits its document, and the person's own words stay when it edits again", async ({ yap, main, shot }) => {
  // (Outline off: this one is about the edits.)
  await setConfig(yap, main, { chatOutlineReview: false });
  const DUMP = fresh('edit.jsonl');
  const ARGS = fresh('edit-args.jsonl');
  const intro = block('p', WORDS.Introduction);
  const what = block('p', WORDS['What goes in']);
  const trouble = block('l', WORDS.Troubleshooting);
  await play(yap, 'edit', [
    makes(DUMP, null),
    // "Make the introduction shorter": read it, then that block replaced.
    turn(
      null,
      yapCall('m10', 'toolu_read', 'read_document', { note_id: DOC }, DUMP),
      yapCall('m11', 'toolu_short', 'edit_document', { note_id: DOC, ops: [{ op: 'replace', id: intro.id, if_hash: intro.hash, markdown: SHORT_INTRO }] }, DUMP),
      answers('m12', "I've made the introduction shorter.")
    ),
    // After the person typed into "What goes in": a stale replace of their block is refused; a tip added to the list goes in.
    turn(
      ARGS,
      yapCall('m13', 'toolu_stale', 'edit_document', { note_id: DOC, ops: [{ op: 'replace', id: what.id, if_hash: what.hash, markdown: 'Mix greens with browns.' }] }, DUMP),
      yapCall('m14', 'toolu_tip', 'edit_document', { note_id: DOC, ops: [{ op: 'replace', id: trouble.id, if_hash: trouble.hash, markdown: `${WORDS.Troubleshooting}\n${TIP}` }] }, DUMP),
      answers('m15', "I've added a tip on turning the heap.")
    ),
  ]);
  await freshChat(main);
  await send(main, 'Write me a one-page guide to composting at home as a document');
  await expect(lastAnswer(main)).toHaveText("It's open on the right.", { timeout: 30_000 });
  await noteIs(yap, TITLE, written());

  // The edit: that block only, a version of Chat's (its Undo), the panel showing it.
  await send(main, 'Make the introduction shorter');
  await expect(lastAnswer(main)).toHaveText("I've made the introduction shorter.", { timeout: 30_000 });
  const shorter = written().replace(WORDS.Introduction, SHORT_INTRO);
  await noteIs(yap, TITLE, shorter);
  await expect(docBody(main)).toContainText(SHORT_INTRO);
  const read = resultOf(linesOf(DUMP)[4]);
  expect(read.success).toBe(true);
  expect(read.data.blocks.find((b) => b.id === intro.id)?.hash).toBe(intro.hash);
  const short = resultOf(linesOf(DUMP)[5]);
  expect(short).toMatchObject({ success: true, data: { changed: true } });
  await expect(chipsOfLast(main).last()).toContainText(TITLE);
  await expect(panel(main).getByRole('button', { name: "Undo Chat's last change" })).toBeVisible();
  await shot(main, '05-plan-document-edited-light');

  // The person types into the page: their sentence, saved (merged with Chat's writing).
  await expect(page(main)).toHaveAttribute('contenteditable', 'true');
  await page(main).locator('p', { hasText: 'Mix greens' }).click();
  await main.keyboard.press('End');
  await main.waitForTimeout(80);
  await main.keyboard.type(TYPED);
  const typed = shorter.replace(WORDS['What goes in'], WORDS['What goes in'] + TYPED);
  await noteIs(yap, TITLE, typed);

  // Another edit: Claude's stale change to their block is refused, the tip goes in, their words stay.
  await send(main, 'Now add a tip about turning the heap');
  await expect(lastAnswer(main)).toHaveText("I've added a tip on turning the heap.", { timeout: 30_000 });
  const tipped = typed.replace(WORDS.Troubleshooting, `${WORDS.Troubleshooting}\n${TIP}`);
  await noteIs(yap, TITLE, tipped);
  const [stale, tip] = linesOf(DUMP).slice(6);
  expect(stale.isError).toBe(true);
  expect(resultOf(stale).error).toContain('Nothing was changed');
  expect(resultOf(tip)).toMatchObject({ success: true, data: { changed: true } });
  await expect(chipsOfLast(main).first()).toContainText(`Didn't change "${TITLE}"`);
  await expect(docBody(main)).toContainText(TYPED.trim());
  // The question told Claude what they changed, as the tool loop's does (documents::context).
  const asked = linesOf(ARGS).at(-1).user_text;
  expect(asked).toContain(`Since you last saw "${TITLE}", the user changed it`);
  expect(asked).toContain('Crushed eggshells');
  await shot(main, '06-plan-document-persons-words-kept-light');
  await theme(yap, main, 'dark');
  await shot(main, '07-plan-document-persons-words-kept-dark');
  await theme(yap, main, 'light');
});

test("Edit on the plan's outline: the writing follows the person's sections, and the model reads what changed", async ({ yap, main }) => {
  const DUMP = fresh('outline-edit.jsonl');
  // What Claude Code writes once told: the sections as the person left them.
  const theirs = { ...OUTLINE, sections: [OUTLINE.sections[1], OUTLINE.sections[0]] };
  await play(yap, 'outline-edit', [
    turn(
      null,
      yapCall('m1', 'toolu_doc', 'create_document', OUTLINE, DUMP),
      theirs.sections.map((s, i) => yapCall(`m${i + 2}`, `toolu_s${i}`, 'write_section', { note_id: DOC, key: s.key, markdown: WORDS[s.key] }, DUMP)),
      answers('m9', "It's open on the right.")
    ),
  ]);
  await freshChat(main);
  await send(main, 'Make me a composting guide to keep');
  await expect(card(main)).toHaveAttribute('data-state', 'asking', { timeout: 20_000 });
  await card(main).getByRole('button', { name: 'Edit' }).click();
  // "What goes in" up above the introduction; Troubleshooting out; a note.
  await card(main).getByRole('button', { name: 'Move section 2 up' }).click();
  await card(main).getByRole('button', { name: 'Take out section 3' }).click();
  await card(main).getByTestId('chat-outline-note').fill('Keep it to one page.');
  await card(main).getByRole('button', { name: 'Go with these changes' }).click();
  await expect(lastAnswer(main)).toHaveText("It's open on the right.", { timeout: 30_000 });
  await expect(docBody(main).locator('h3')).toHaveText(['What goes in', 'Introduction']);
  await expect(card(main)).toContainText('Written with your changes');
  const told = resultOf(linesOf(DUMP)[0]);
  expect(told.data.sections).toEqual(['What goes in', 'Introduction']);
  expect(told.data.outline).toContain('The user changed the outline before you wrote it');
  expect(told.data.outline).toContain('“Keep it to one page.”');
  await noteIs(yap, TITLE, written(theirs));
});
