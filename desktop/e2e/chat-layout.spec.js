// Chat's layout (desktop/src/lib/views/chat: chatRoom.js, the rules, pure;
// chatLayout.svelte.js, its state; ChatView, ConversationList), 7 Nov 2026,
// after the Claude app's sidebar button and its split:
//
// - chatRoom.js with no app: what gives way first as the room runs out (the
//   list, then the panel's width, then the panel covering the conversation),
//   the list's button, the split's keys.
// - **The list's button**: at the list's top left ("Hide your chats ·
//   Ctrl+B"), at the thread's while it's hidden ("Show your chats"); Ctrl+B
//   anywhere in Chat but rich text; the focus following the button; the
//   person's choice remembered across a reload. Light and dark, the Code
//   side's switch beside it.
// - **Ctrl+B in a document beside the chat** makes the words bold (the
//   note's markdown), the list as it was; elsewhere it's the list's.
// - **The split**: dragged (the width follows, kept; the panel's 320 and the
//   thread's 340 held), Esc mid-drag putting it back, the arrow keys
//   (Shift: further), Home / End, a double-click back to 460; remembered when
//   the panel opens again. Mid-drag in light and dark.
// - **A job starting** (the fake Claude Code, as jobs-ui.spec.js): its tab
//   opens at the remembered width in the same frame the list steps aside —
//   every box measured each frame: one change, never an intermediate width,
//   the composer's height the same throughout (a chat turning into a Code
//   chat), the thread still at its newest line. Light and dark.
// - **Narrow windows**: the panel's width gives way to the thread's least;
//   the list asked back with no room beside both floats over the thread (Esc,
//   a press in the thread put it aside); narrower still, the panel covers
//   the conversation and Ctrl+B leaves the list alone.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, setConfig, expectStore } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { RUNS } from './support/yap.js';
import { toSide } from './support/code-chats.js';
import { chatRoom, toggleList, splitKeyWidth, MIN_WIDTH, DEFAULT_WIDTH, THREAD_MIN, LIST_STAYS, LIST_WIDTH } from '../src/lib/views/chat/chatRoom.js';

const HERE = path.join(RUNS, 'chat-layout-spec');
const SCRIPTS = path.join(HERE, 'scripts');
const GARDEN = path.join(HERE, 'folders', 'garden-planner');

// ---- the fake Claude Code's script (as jobs-ui.spec.js has them): a job that takes its time ----
const init = {
  type: 'system',
  subtype: 'init',
  cwd: '{{cwd}}',
  session_id: '{{session_id}}',
  tools: ['Bash', 'Edit', 'Read', 'Write'],
  mcp_servers: [{ name: 'yap', status: 'connected' }],
  model: 'claude-sonnet-5-5',
  permissionMode: 'auto',
  apiKeySource: 'none',
  claude_code_version: '2.1.296',
  capabilities: ['interrupt_receipt_v1'],
};
const SLOW = [
  { _wait_user: true },
  init,
  {
    type: 'assistant',
    message: { id: 's1', type: 'message', role: 'assistant', model: 'claude-sonnet-5-5', content: [{ type: 'tool_use', id: 'toolu_read', name: 'Read', input: { file_path: '{{cwd}}\\notes.md' } }], stop_reason: 'tool_use' },
    parent_tool_use_id: null,
    session_id: '{{session_id}}',
  },
  { type: 'user', message: { role: 'user', content: [{ type: 'tool_result', tool_use_id: 'toolu_read', content: 'Draft notes' }] }, parent_tool_use_id: null, session_id: '{{session_id}}' },
  { _sleep_ms: 60000 },
  { type: 'result', subtype: 'success', is_error: false, duration_ms: 1, num_turns: 1, result: 'Stopped.', session_id: '{{session_id}}', total_cost_usd: 0, usage: {}, permission_denials: [], terminal_reason: 'completed' },
  { _wait_user: true },
];

function writeScript(name, lines) {
  fs.mkdirSync(SCRIPTS, { recursive: true });
  const file = path.join(SCRIPTS, `${name}.jsonl`);
  fs.writeFileSync(file, lines.map((l) => JSON.stringify(l)).join('\n') + '\n');
  return file;
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
  yapOptions: [async ({}, use) => use({ name: 'chat-layout', config: { memoryLearnChats: false, meetingReminder: 'never' } }), { scope: 'worker' }],
});

const box = (main) => main.getByRole('textbox', { name: 'Message' });
const msgs = (main) => main.locator('.msgs');
const list = (main) => main.locator('.convs');
const hideBtn = (main) => main.getByTestId('chat-list-toggle');
const showBtn = (main) => main.getByTestId('chat-list-show');
const panel = (main) => main.getByTestId('chat-doc');
const slot = (main) => main.getByTestId('chat-panel-slot');
const split = (main) => main.getByRole('separator', { name: 'Resize the panel' });
const jobTab = (main) => main.getByTestId('chat-job');
const editor = (main) => panel(main).locator('.docedit');
const threadBox = (main) => main.locator('section.thread');

/** An element's box (CSS px, rounded). */
const rect = (loc) => loc.evaluate((el) => {
  const r = el.getBoundingClientRect();
  return { x: Math.round(r.x), y: Math.round(r.y), w: Math.round(r.width), h: Math.round(r.height), right: Math.round(r.right) };
});
/** Chat's own width (`.chat`, what chatRoom is given). */
const chatWidth = (main) => main.locator('.chat').evaluate((el) => el.clientWidth);

/** Everything drawn in the composer (its box, its buttons, its chips) inside the thread. */
async function expectComposerInside(main) {
  const thread = await rect(threadBox(main));
  const out = await main.locator('.composer').evaluate((el, right) => {
    const shown = [...el.querySelectorAll('button, .inputbar, textarea')].filter((e) => e.getClientRects().length && getComputedStyle(e).visibility !== 'hidden');
    return shown.filter((e) => e.getBoundingClientRect().right > right - 20 + 0.5).map((e) => e.getAttribute('aria-label') || e.className);
  }, thread.right);
  // (The thread's own padding is 20 px: nothing reaches into it.)
  expect(out, 'cut off at the thread’s right edge').toEqual([]);
  // The foot's chips side by side, none drawn over the next (their words clipped to their pills).
  const overlaps = await main.locator('.composerfoot').evaluate((foot) => {
    const boxes = [...foot.querySelectorAll('button')]
      .filter((e) => e.getClientRects().length && getComputedStyle(e).visibility !== 'hidden')
      .map((e) => ({ name: e.getAttribute('aria-label') || e.textContent.trim(), r: e.getBoundingClientRect(), clips: getComputedStyle(e).overflow !== 'visible' || e.scrollWidth <= e.clientWidth + 1 }))
      .sort((a, b) => a.r.left - b.r.left);
    const bad = [];
    for (let i = 0; i < boxes.length; i++) {
      if (!boxes[i].clips) bad.push(`${boxes[i].name}: its words spill out`);
      if (i && boxes[i].r.left < boxes[i - 1].r.right - 0.5) bad.push(`${boxes[i].name} over ${boxes[i - 1].name}`);
    }
    return bad;
  });
  expect(overlaps, 'the composer’s chips').toEqual([]);
}

async function send(main, text) {
  await box(main).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
}

async function theme(yap, main, which) {
  await setConfig(yap, main, { theme: which });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe(which);
}

/** This window's own layout choices forgotten (the list shown at its width, the split at its default; the app's rail expanded). */
async function forgetLayout(main) {
  await main.evaluate(() => {
    localStorage.removeItem('yapChatListHidden');
    localStorage.removeItem('yapChatListWidth');
    localStorage.removeItem('yapChatDocWidth');
    localStorage.removeItem('yapNavCollapsed');
  });
}

/** Chat shown afresh (its layout read again from this window's storage). */
async function reopenChat(main) {
  await openView(main, 'Home');
  await openView(main, 'Chat');
}

/** The document `create_document` made earlier in this turn. */
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

/** A new plain chat whose answer writes document `title` beside it (its page editable once written). */
async function openDocument(main, fakeLlm, title) {
  fakeLlm.scriptChat([
    { tools: [{ name: 'create_document', args: { title, sections: [{ key: 'Beds', intent: 'what goes where' }] } }] },
    { tools: [{ name: 'write_section', args: (r) => ({ note_id: madeDoc(r), key: 'Beds', markdown: 'Beans by the fence, peas by the shed.' }) }] },
    { content: "It's open on the right." },
  ]);
  await openView(main, 'Chat');
  await toSide(main, 'Chat');
  if (await showBtn(main).isVisible()) await showBtn(main).click();
  await main.getByRole('button', { name: 'New chat' }).click();
  await send(main, `Plan my ${title.toLowerCase()}`);
  await expect(msgs(main)).toContainText("It's open on the right.", { timeout: 20_000 });
  await expect(editor(main)).toHaveAttribute('contenteditable', 'true');
}

test.beforeEach(async ({ yap, main, fakeLlm }) => {
  await main.setViewportSize({ width: 1200, height: 800 });
  await forgetLayout(main);
  fs.rmSync(GARDEN, { recursive: true, force: true });
  fs.mkdirSync(GARDEN, { recursive: true });
  fs.writeFileSync(path.join(GARDEN, 'notes.md'), 'Draft notes\n');
  await yap.invoke('e2e_claude_approve_auto', { allow: true });
  await setConfig(yap, main, {
    jobsEnabled: true,
    jobsAsk: 'new-or-web',
    jobsSpeak: 'never',
    jobsPay: 'plan',
    jobsMode: 'auto',
    webSearch: false,
    speakReplies: false,
    llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat', apiKey: '', apiKeys: {}, prompt: '', disableThinking: false } },
  });
  if (!(await yap.invoke('job_folders_list')).some((f) => f.name === 'garden-planner')) await yap.invoke('job_folders_add', { path: GARDEN });
});

test.afterEach(async ({ main }) => {
  await main.setViewportSize({ width: 1200, height: 800 });
  await forgetLayout(main);
});

// ---------------------------------------------------------------- no app

test('chatRoom.js: the list, then the panel’s width, then the panel covering; the button; the split’s keys', () => {
  // No panel: the list as the person left it.
  expect(chatRoom({ chat: 966 })).toMatchObject({ listShown: true, listFloats: false, cover: false });
  expect(chatRoom({ chat: 966, hidden: true })).toMatchObject({ listShown: false });
  // A panel in a Chat narrower than LIST_STAYS: the list steps aside (Yap's,
  // not remembered), the panel at its remembered width.
  const job = chatRoom({ chat: 966, panelOpen: true, want: 520 });
  expect(job).toMatchObject({ listShown: false, listAuto: true, cover: false, panelWidth: 520, maxWidth: 966 - THREAD_MIN });
  // Wide enough: the list stays beside both.
  expect(chatRoom({ chat: LIST_STAYS, panelOpen: true, want: 520 })).toMatchObject({ listShown: true, listAuto: false, panelWidth: 520 });
  // Brought back with room beside both: the panel gives way, the thread keeps 340.
  const back = chatRoom({ chat: 966, panelOpen: true, want: 520, shownForPanel: true });
  expect(back).toMatchObject({ listShown: true, listFloats: false, panelWidth: 966 - LIST_WIDTH - THREAD_MIN });
  // …and with no room beside both: it floats over the thread.
  expect(chatRoom({ chat: 766, panelOpen: true, want: 520, shownForPanel: true })).toMatchObject({ listShown: true, listFloats: true, panelWidth: 766 - THREAD_MIN });
  // The panel's width gives way down to its least; then it covers the conversation.
  expect(chatRoom({ chat: 666, panelOpen: true, want: 520 })).toMatchObject({ panelWidth: 326, cover: false, narrow: false });
  expect(chatRoom({ chat: 646, panelOpen: true, want: 520 })).toMatchObject({ cover: true, narrow: true, panelWidth: MIN_WIDTH });
  // Expanded: covers; the list behind it never moves.
  expect(chatRoom({ chat: 1300, panelOpen: true, expanded: true })).toMatchObject({ cover: true, listShown: true });
  // Not measured yet: nothing steps aside.
  expect(chatRoom({ chat: 0, panelOpen: true })).toMatchObject({ listShown: true, narrow: false, panelWidth: DEFAULT_WIDTH });

  // The button: shown → the person's hide; hidden → shown, over a panel too.
  expect(toggleList({ listShown: true, panelOpen: false })).toEqual({ hidden: true, shownForPanel: false });
  expect(toggleList({ listShown: false, panelOpen: true })).toEqual({ hidden: false, shownForPanel: true });
  expect(toggleList({ listShown: false, panelOpen: false })).toEqual({ hidden: false, shownForPanel: false });

  // The split's keys: ← wider, → narrower (Shift: further), Home / End, held to the room.
  const at = { width: 460, maxWidth: 600 };
  expect(splitKeyWidth('ArrowLeft', at)).toBe(484);
  expect(splitKeyWidth('ArrowRight', { ...at, shift: true })).toBe(380);
  expect(splitKeyWidth('Home', at)).toBe(600);
  expect(splitKeyWidth('End', at)).toBe(MIN_WIDTH);
  expect(splitKeyWidth('ArrowLeft', { width: 590, maxWidth: 600 })).toBe(600);
  expect(splitKeyWidth('ArrowRight', { width: 330, maxWidth: 600 })).toBe(MIN_WIDTH);
  expect(splitKeyWidth('a', at)).toBeNull();
});

// ---------------------------------------------------------------- the list's button

test('the list’s button and Ctrl+B hide and show the list, the focus following; remembered across a reload', async ({ yap, main, shot }) => {
  await openView(main, 'Chat');
  await toSide(main, 'Code');
  await expect(list(main)).toBeVisible();
  await expect(hideBtn(main)).toHaveAttribute('title', 'Hide your chats · Ctrl+B');
  await expect(hideBtn(main)).toHaveAttribute('aria-keyshortcuts', 'Control+B');
  // At the list's top left, beside the Chat | Code switch; the list's width fixed on both sides.
  const listBox = await rect(list(main));
  const btn = await rect(hideBtn(main));
  expect(btn.x - listBox.x).toBeLessThanOrEqual(10);
  expect(listBox.w).toBe(LIST_WIDTH);
  await expect(main.getByRole('button', { name: 'New code chat' })).toBeVisible();
  await shot(main, '01-list-shown-light');
  await theme(yap, main, 'dark');
  await shot(main, '02-list-shown-dark');
  await theme(yap, main, 'light');
  await toSide(main, 'Chat');
  expect((await rect(list(main))).w).toBe(LIST_WIDTH);

  // The button: hidden; its other place at the thread's top left, the focus on it.
  const threadBefore = await rect(threadBox(main));
  await hideBtn(main).click();
  await expect(list(main)).toBeHidden();
  await expect(showBtn(main)).toBeVisible();
  await expect(showBtn(main)).toBeFocused();
  await expect(showBtn(main)).toHaveAttribute('title', 'Show your chats · Ctrl+B');
  await expect(showBtn(main)).toHaveAttribute('aria-expanded', 'false');
  // The chat gets the room; the button sits where the list's was.
  const threadHidden = await rect(threadBox(main));
  expect(threadHidden.x).toBe(listBox.x);
  expect(threadHidden.w).toBe(threadBefore.w + LIST_WIDTH);
  const shown = await rect(showBtn(main));
  expect(Math.abs(shown.x - btn.x)).toBeLessThanOrEqual(1);
  expect(Math.abs(shown.y - btn.y)).toBeLessThanOrEqual(1);
  await shot(main, '03-list-hidden-light');
  await theme(yap, main, 'dark');
  await shot(main, '04-list-hidden-dark');
  await theme(yap, main, 'light');
  // Enter on it: back, the focus on the list's own.
  await main.keyboard.press('Enter');
  await expect(list(main)).toBeVisible();
  await expect(hideBtn(main)).toBeFocused();

  // Ctrl+B from the message box: hidden and shown, the box keeping the focus.
  await box(main).click();
  await main.keyboard.press('Control+b');
  await expect(list(main)).toBeHidden();
  await expect(box(main)).toBeFocused();
  await main.keyboard.press('Control+b');
  await expect(list(main)).toBeVisible();
  await expect(box(main)).toBeFocused();

  // Not from a dialog over Chat (the shortcut sheet lists it).
  await main.keyboard.press('Control+/');
  const sheet = main.getByRole('dialog', { name: 'Keyboard shortcuts' });
  await expect(sheet).toBeVisible();
  await expect(sheet.getByRole('region', { name: "In Yap's window" })).toContainText('Hide or show your chats, in Chat');
  await main.keyboard.press('Control+b');
  await expect(list(main)).toBeVisible();
  await main.keyboard.press('Escape');
  await expect(sheet).toHaveCount(0);

  // Hidden, then a reload: still hidden. Shown, a reload: shown.
  await main.keyboard.press('Control+b');
  await expect(list(main)).toBeHidden();
  expect(await main.evaluate(() => localStorage.getItem('yapChatListHidden'))).toBe('1');
  await main.reload({ waitUntil: 'load' });
  await openView(main, 'Chat');
  await expect(showBtn(main)).toBeVisible();
  await expect(list(main)).toBeHidden();
  await showBtn(main).click();
  await expect(list(main)).toBeVisible();
  await main.reload({ waitUntil: 'load' });
  await openView(main, 'Chat');
  await expect(list(main)).toBeVisible();
  await expect(showBtn(main)).toHaveCount(0);
});

// ---------------------------------------------------------------- a document, the split

test('Ctrl+B in a document beside the chat makes the words bold; the split drags, keys and resets, remembered', async ({ yap, main, fakeLlm, shot }) => {
  await openDocument(main, fakeLlm, 'Garden beds');
  const width = await chatWidth(main);
  // Chat isn't LIST_STAYS wide: the list stepped aside for the document, its button showing.
  expect(width).toBeLessThan(LIST_STAYS);
  await expect(list(main)).toBeHidden();
  await expect(showBtn(main)).toBeVisible();
  expect((await rect(slot(main))).w).toBe(DEFAULT_WIDTH);

  // Ctrl+B with a word selected on the page: bold, saved as markdown; the list as it was.
  await editor(main).locator('p', { hasText: 'Beans by the fence' }).dblclick({ position: { x: 12, y: 8 } });
  // (The editor takes the page's selection in a beat.)
  await main.waitForTimeout(80);
  await main.keyboard.press('Control+b');
  await expectStore(yap, 'notes.json', (s) => s.notes.find((n) => n.title === 'Garden beds')?.content?.includes('**Beans**'), 'the word bold in the note');
  await expect(list(main)).toBeHidden();
  await expect(showBtn(main)).toBeVisible();
  // From the message box: the list's — back beside both (the panel gives way, the thread keeps 340).
  await box(main).click();
  await main.keyboard.press('Control+b');
  await expect(list(main)).toBeVisible();
  const back = chatRoom({ chat: width, panelOpen: true, want: DEFAULT_WIDTH, shownForPanel: true });
  expect(back.listFloats).toBe(false);
  await expect.poll(async () => (await rect(slot(main))).w).toBe(back.panelWidth);
  expect((await rect(threadBox(main))).w).toBeGreaterThanOrEqual(THREAD_MIN);
  // Hidden again by the person: remembered, the list stays hidden when the panel closes.
  await main.keyboard.press('Control+b');
  await expect(list(main)).toBeHidden();
  expect(await main.evaluate(() => localStorage.getItem('yapChatListHidden'))).toBe('1');
  await panel(main).getByRole('button', { name: 'Close the document' }).click();
  await expect(panel(main)).toHaveCount(0);
  await expect(list(main)).toBeHidden();
  await showBtn(main).click();
  await expect(list(main)).toBeVisible();
  await main.locator('[data-testid="chat-doc-pill"]').filter({ hasText: 'Garden beds' }).click();
  await expect(panel(main)).toBeVisible();
  // (Stepped aside again for it: Yap's, never remembered.)
  await expect(list(main)).toBeHidden();
  expect(await main.evaluate(() => localStorage.getItem('yapChatListHidden'))).toBeNull();

  // **The split**: a drag 100 px to the left widens the panel by 100.
  const s = split(main);
  await expect(s).toHaveAttribute('title', 'Drag to resize · Double-click to reset');
  const room = chatRoom({ chat: width, panelOpen: true, want: DEFAULT_WIDTH });
  await expect(s).toHaveAttribute('aria-valuemax', String(room.maxWidth));
  const handle = await rect(s);
  const cx = handle.x + handle.w / 2;
  const cy = handle.y + handle.h / 2;
  await main.mouse.move(cx, cy);
  await main.mouse.down();
  await main.mouse.move(cx - 60, cy, { steps: 4 });
  await main.mouse.move(cx - 100, cy, { steps: 4 });
  // Held: the resize cursor everywhere in Chat, the grip lit.
  await expect(main.locator('.chat')).toHaveClass(/\bresizing\b/);
  expect(await threadBox(main).evaluate((el) => getComputedStyle(el).cursor)).toBe('col-resize');
  await expect.poll(async () => (await rect(slot(main))).w).toBe(DEFAULT_WIDTH + 100);
  await shot(main, '05-split-mid-drag-light');
  await theme(yap, main, 'dark');
  await shot(main, '06-split-mid-drag-dark');
  await theme(yap, main, 'light');
  await main.mouse.up();
  await expect(main.locator('.chat')).not.toHaveClass(/\bresizing\b/);
  await expect(s).toHaveAttribute('aria-valuenow', String(DEFAULT_WIDTH + 100));
  expect(await main.evaluate(() => localStorage.getItem('yapChatDocWidth'))).toBe(String(DEFAULT_WIDTH + 100));

  // Esc mid-drag: back where it was.
  const now = await rect(s);
  await main.mouse.move(now.x + now.w / 2, cy);
  await main.mouse.down();
  await main.mouse.move(now.x + now.w / 2 + 120, cy, { steps: 4 });
  await expect.poll(async () => (await rect(slot(main))).w).toBe(DEFAULT_WIDTH - 20);
  await main.keyboard.press('Escape');
  await main.mouse.up();
  await expect.poll(async () => (await rect(slot(main))).w).toBe(DEFAULT_WIDTH + 100);
  // (Esc was the split's: the panel is still there.)
  await expect(panel(main)).toBeVisible();

  // Its least held: dragged far right, 320; far left, the thread keeps 340.
  let h = await rect(s);
  await main.mouse.move(h.x + h.w / 2, cy);
  await main.mouse.down();
  await main.mouse.move(h.x + 600, cy, { steps: 6 });
  await main.mouse.up();
  await expect.poll(async () => (await rect(slot(main))).w).toBe(MIN_WIDTH);
  h = await rect(s);
  await main.mouse.move(h.x + h.w / 2, cy);
  await main.mouse.down();
  await main.mouse.move(5, cy, { steps: 8 });
  await main.mouse.up();
  await expect.poll(async () => (await rect(slot(main))).w).toBe(room.maxWidth);
  expect((await rect(threadBox(main))).w).toBe(THREAD_MIN);
  // The composer and its chips never cut off at the thread's least.
  await expectComposerInside(main);
  // …nor the panel's buttons at its least.
  await s.focus();
  await main.keyboard.press('End');
  await expect(s).toHaveAttribute('aria-valuenow', String(MIN_WIDTH));
  const p = await rect(panel(main));
  for (const name of ['More for the document', 'Close the document']) {
    const r = await rect(panel(main).getByRole('button', { name }));
    expect(r.right, `${name} inside the panel`).toBeLessThanOrEqual(p.right);
  }

  // The keys: ← wider (Shift: further), → narrower, Home the widest.
  await main.keyboard.press('ArrowLeft');
  await expect(s).toHaveAttribute('aria-valuenow', String(MIN_WIDTH + 24));
  await main.keyboard.press('Shift+ArrowLeft');
  await expect(s).toHaveAttribute('aria-valuenow', String(MIN_WIDTH + 104));
  await main.keyboard.press('ArrowRight');
  await expect(s).toHaveAttribute('aria-valuenow', String(MIN_WIDTH + 80));
  await main.keyboard.press('Home');
  await expect(s).toHaveAttribute('aria-valuenow', String(room.maxWidth));
  // A double-click: the default again.
  await s.dblclick();
  await expect(s).toHaveAttribute('aria-valuenow', String(DEFAULT_WIDTH));
  await expect.poll(async () => (await rect(slot(main))).w).toBe(DEFAULT_WIDTH);

  // Remembered: 400, then the panel closed and opened again (and Chat itself).
  await main.keyboard.press('Shift+ArrowRight');
  await expect(s).toHaveAttribute('aria-valuenow', '380');
  await panel(main).getByRole('button', { name: 'Close the document' }).click();
  await expect(panel(main)).toHaveCount(0);
  await reopenChat(main);
  await main.locator('.conv').filter({ hasText: 'Plan my garden beds' }).click();
  await main.locator('[data-testid="chat-doc-pill"]').filter({ hasText: 'Garden beds' }).click();
  await expect(panel(main)).toBeVisible();
  expect((await rect(slot(main))).w).toBe(380);
});

// ---------------------------------------------------------------- a job starting

/** Every box that could move, each frame from now on (`window.__layout`). */
function recordFrames() {
  const q = (s) => document.querySelector(s);
  const box = (el) => {
    if (!el) return null;
    const r = el.getBoundingClientRect();
    return r.width || r.height ? [Math.round(r.x), Math.round(r.width), Math.round(r.height)] : null;
  };
  const rec = { frames: [], on: true };
  window.__layout = rec;
  const step = () => {
    const m = q('.msgs');
    rec.frames.push({
      t: Math.round(performance.now()),
      list: box(q('.convs')),
      thread: box(q('section.thread')),
      slot: box(q('[data-testid="chat-panel-slot"]')),
      panel: box(q('[data-testid="chat-doc"]')),
      job: box(q('[data-testid="chat-job"]')),
      composer: box(q('.composer')),
      atBottom: m ? m.scrollHeight - m.clientHeight - m.scrollTop < 4 : null,
      scrolls: m ? m.scrollHeight > m.clientHeight + 4 : null,
    });
    if (rec.on) requestAnimationFrame(step);
  };
  requestAnimationFrame(step);
}

for (const where of ['plain', 'code']) {
  test(`a job starting from a ${where} chat: its tab opens at the remembered width, in one frame, nothing else moving`, async ({ yap, main, fakeLlm, shot }) => {
    // The panel was 520 wide last time.
    await main.evaluate(() => localStorage.setItem('yapChatDocWidth', '520'));
    await reopenChat(main);
    await yap.invoke('e2e_claude_script', { script: writeScript('slow', SLOW) });
    fakeLlm.scriptChat([
      // (Long, and never the same line twice: an answer repeating itself is cut, chat/stream.rs.)
      { content: Array.from({ length: 40 }, (_, i) => `Bed ${i + 1} gets ${['beans', 'peas', 'kale', 'leeks', 'chard'][i % 5]}, ${(i % 7) + 1} litres a week, planted in week ${(i * 3) % 13 + 1}.`).join('\n\n') },
      { tools: [{ name: 'start_job', args: { folder: 'garden-planner', brief: 'Read notes.md slowly.' } }] },
      { content: "On it: I've started a job in garden-planner." },
    ]);
    if (where === 'plain') {
      await toSide(main, 'Chat');
      await main.getByRole('button', { name: 'New chat' }).click();
    } else {
      await toSide(main, 'Code');
      await main.getByRole('button', { name: 'New code chat' }).click();
      await main.getByTestId('chat-workspace-menu').getByRole('menuitemradio', { name: 'garden-planner', exact: true }).click();
    }
    await send(main, 'Tell me about the garden planner first.');
    await expect(main.locator('.cbubble.assistant').last()).toContainText('Bed 40 gets chard');
    // At its newest line, and long enough to scroll.
    await expect.poll(() => msgs(main).evaluate((m) => m.scrollHeight - m.clientHeight - m.scrollTop)).toBeLessThan(4);

    await main.evaluate(recordFrames);
    await send(main, 'Start a job in garden-planner.');
    await expect(jobTab(main)).toBeVisible();
    await shot(main, `07-job-tab-opening-${where}`);
    await expect(main.locator('.cbubble.assistant').last()).toContainText("On it: I've started a job in garden-planner.");
    await main.waitForTimeout(600);
    const frames = await main.evaluate(() => {
      window.__layout.on = false;
      return window.__layout.frames;
    });
    const key = (b) => JSON.stringify(b);
    const changes = (field) => frames.filter((f, i) => i > 0 && key(f[field]) !== key(frames[i - 1][field]));

    // The slot: from its first frame at the remembered width, never another;
    // the panel and its Job tab drawn in that very frame (no empty gap).
    const first = frames.findIndex((f) => f.slot);
    expect(first, 'the panel opened').toBeGreaterThan(0);
    const opened = frames[first];
    expect(opened.slot[1]).toBe(520);
    expect(opened.panel?.[1], 'the panel in its first frame').toBe(520);
    expect(opened.job, 'the Job tab in its first frame').not.toBeNull();
    expect(new Set(frames.slice(first).map((f) => key([f.slot[0], f.slot[1]]))).size, 'the panel never moves').toBe(1);
    // The list stepped aside in that same frame, and the thread changed once:
    // no intermediate width before or after.
    expect(frames[first - 1].list?.[1]).toBe(LIST_WIDTH);
    expect(opened.list).toBeNull();
    const threadMoves = changes('thread');
    expect(threadMoves.map((f) => f.t), 'the thread changed once, as the panel opened').toEqual([opened.t]);
    expect(opened.thread[1]).toBe((await chatWidth(main)) - 520);
    // The composer as tall throughout (a plain chat turning into a Code chat too).
    expect(new Set(frames.map((f) => f.composer[2])).size, 'the composer’s height').toBe(1);
    // The thread kept its newest line in view.
    expect(frames.at(-1).scrolls).toBe(true);
    expect(frames.at(-1).atBottom).toBe(true);
    // The list's button says what happened: back with one click.
    await expect(showBtn(main)).toBeVisible();

    if (where === 'code') {
      await theme(yap, main, 'dark');
      await shot(main, '08-job-tab-opening-dark');
      await theme(yap, main, 'light');
      // A Code chat at the thread's least (the workspace and Mode chips): nothing cut off.
      await split(main).focus();
      await main.keyboard.press('Home');
      await expect.poll(async () => (await rect(threadBox(main))).w).toBe(THREAD_MIN);
      await expectComposerInside(main);
      await shot(main, '11-code-chat-at-the-thread-least');
      // The Job tab at the panel's least: its buttons all there.
      await main.keyboard.press('End');
      await expect.poll(async () => (await rect(slot(main))).w).toBe(MIN_WIDTH);
      const p = await rect(jobTab(main));
      for (const name of ['Stop', 'Close the panel']) {
        const r = await rect(jobTab(main).getByRole('button', { name, exact: true }).first());
        expect(r.right, `${name} inside the Job tab`).toBeLessThanOrEqual(p.right);
      }
      await shot(main, '12-job-tab-at-its-least');
    }
    await jobTab(main).getByRole('button', { name: 'Stop' }).click();
    await expect(main.getByTestId('chat-job-status')).toHaveText('Stopped', { timeout: 20_000 });
    await jobTab(main).getByRole('button', { name: 'Close the panel' }).click();
    await expect(list(main)).toBeVisible();
  });
}

// ---------------------------------------------------------------- narrow windows

test('a narrow window: the panel gives way, the list floats when asked back, then the panel covers', async ({ yap, main, fakeLlm, shot }) => {
  // (The window's least, 860: the app's rail has narrowed for the panel first, main-layout.spec.js.)
  await main.setViewportSize({ width: 860, height: 760 });
  await openDocument(main, fakeLlm, 'Window boxes');
  const width = await chatWidth(main);
  const room = chatRoom({ chat: width, panelOpen: true, want: DEFAULT_WIDTH });
  // The list first (stepped aside), then the panel's width (the thread keeps its least).
  expect(room.panelWidth).toBeLessThan(DEFAULT_WIDTH);
  await expect(list(main)).toBeHidden();
  await expect.poll(async () => (await rect(slot(main))).w).toBe(room.panelWidth);
  expect((await rect(threadBox(main))).w).toBe(THREAD_MIN);
  // Asked back with no room beside both: over the thread, nothing beside it moving.
  const before = { thread: await rect(threadBox(main)), slot: await rect(slot(main)) };
  await showBtn(main).click();
  await expect(list(main)).toBeVisible();
  await expect(main.locator('.listslot')).toHaveClass(/\bfloating\b/);
  expect(await rect(threadBox(main))).toEqual(before.thread);
  expect(await rect(slot(main))).toEqual(before.slot);
  await shot(main, '09-narrow-list-floating');
  // Esc puts it aside; so does a press in the thread.
  await main.keyboard.press('Escape');
  await expect(list(main)).toBeHidden();
  await expect(panel(main)).toBeVisible();
  await showBtn(main).click();
  await expect(list(main)).toBeVisible();
  await msgs(main).click({ position: { x: 280, y: 40 } });
  await expect(list(main)).toBeHidden();
  // Picking a conversation from it: that conversation, the panel closed, the list back beside it.
  await showBtn(main).click();
  await main.getByRole('button', { name: 'New chat' }).click();
  await expect(panel(main)).toHaveCount(0);
  await expect(list(main)).toBeVisible();
  await expect(main.locator('.listslot')).not.toHaveClass(/\bfloating\b/);

  // Narrower still: the panel covers the conversation; Ctrl+B leaves the list alone.
  await main.locator('.conv').filter({ hasText: 'Plan my window boxes' }).click();
  await main.locator('[data-testid="chat-doc-pill"]').filter({ hasText: 'Window boxes' }).click();
  await main.setViewportSize({ width: 700, height: 640 });
  await expect(slot(main)).toHaveClass(/\bcover\b/);
  await expect(split(main)).toHaveCount(0);
  await box(main).evaluate((el) => el.blur());
  await main.keyboard.press('Control+b');
  expect(await main.evaluate(() => localStorage.getItem('yapChatListHidden'))).toBeNull();
  await shot(main, '10-narrow-panel-covers');
  // ✕: the conversation, the list beside it.
  await panel(main).getByRole('button', { name: 'Close the document' }).click();
  await expect(panel(main)).toHaveCount(0);
  await expect(list(main)).toBeVisible();
  await theme(yap, main, 'light');
});
