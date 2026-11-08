// The main window's layout (7 Nov 2026; the maintainer, in Chat's Code side
// full screen: "I can't move the line that shows the thread… hide the
// left-hand side stuff… more space for the code and the thread"):
//
// - core/navRail.js and chatRoom.js with no app: the rail's toggle, its key,
//   what gives way first (the rail, then the list, then the panel's width),
//   the list's width, its keys and its drag.
// - **The rail** (ControlPanel): ☰ at its top and Ctrl+\ collapse the
//   sidebar to its icons in every view, in one short glide (none with motion
//   reduced), every icon where it was; remembered across a reload (Home and
//   Notes); each icon's name as a tooltip only when narrow, ☰'s with its key;
//   a hover never opens it; the bell's count and the meeting setup's badge
//   still there; not from Settings; on the shortcut sheet. Light and dark.
// - **Chat gives way in order** as the window narrows with a document open:
//   the rail (in the panel's frame, nothing gliding, never remembered), then
//   the list, then the panel's width, then the panel covering; the person's
//   ☰ wins for that panel.
// - **The list's edge**: dragged (Esc puts it back; held between 200 and
//   420), a double-click back to 224, its keys, remembered; Ctrl+B still
//   hides it whole; the thread keeps its least in a narrow window.
// - **The readable column**: a wide thread's messages and composer centred
//   on one 768 px column (the scrollbar's half allowed for), with the list
//   hidden or shown, beside a panel; a narrow thread its own width.
// - **Every size**: 900, 1280, 1920 and 2560 px, Home and Chat (both sides,
//   with a document and with an agent's tab): nothing overlaps, nothing cut off.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, setConfig, sidebar, settingsDialog, closeSettings } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { RUNS } from './support/yap.js';
import { toSide } from './support/code-chats.js';
import { RAIL_WIDE, RAIL_NARROW, RAIL_KEY, railToggle, isRailKey } from '../src/lib/core/navRail.js';
import {
  chatRoom,
  listKeyWidth,
  listDragWidth,
  MIN_WIDTH,
  DEFAULT_WIDTH,
  THREAD_MIN,
  LIST_STAYS,
  LIST_WIDTH,
  LIST_MIN,
  LIST_MAX,
} from '../src/lib/views/chat/chatRoom.js';

const HERE = path.join(RUNS, 'main-layout-spec');
const SCRIPTS = path.join(HERE, 'scripts');
const GARDEN = path.join(HERE, 'folders', 'garden-planner');
/** The readable column at the default Text size (app.css --yap-thread-width). */
const COLUMN = 768;
/** The window's frame around Chat beside the rail: the view's margins (2 + 10) and its border (2). */
const FRAME = 14;

// ---- the fake Claude Code's script (as chat-layout.spec.js): an agent that takes its time ----
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
  yapOptions: [async ({}, use) => use({ name: 'main-layout', config: { memoryLearnChats: false, meetingReminder: 'never' } }), { scope: 'worker' }],
});

const rail = (main) => main.getByTestId('nav-rail');
const railBtn = (main) => main.getByTestId('nav-rail-toggle');
const navItem = (main, name) => sidebar(main).getByRole('button', { name, exact: true });
const box = (main) => main.getByRole('textbox', { name: 'Message' });
const msgs = (main) => main.locator('.msgs');
const column = (main) => main.getByTestId('chat-column');
const composer = (main) => main.locator('.composer');
const list = (main) => main.locator('.convs');
const edge = (main) => main.getByRole('separator', { name: 'Resize your chats' });
const showBtn = (main) => main.getByTestId('chat-list-show');
const panel = (main) => main.getByTestId('chat-doc');
const slot = (main) => main.getByTestId('chat-panel-slot');
const editor = (main) => panel(main).locator('.docedit');
const threadBox = (main) => main.locator('section.thread');
const agentTab = (main) => main.getByTestId('chat-agent');

/** An element's box (CSS px, rounded). */
const rect = (loc) =>
  loc.evaluate((el) => {
    const r = el.getBoundingClientRect();
    return { x: Math.round(r.x), y: Math.round(r.y), w: Math.round(r.width), h: Math.round(r.height), right: Math.round(r.right), bottom: Math.round(r.bottom) };
  });
const railWidth = (main) => rail(main).evaluate((el) => Math.round(el.getBoundingClientRect().width));
const chatWidth = (main) => main.locator('.chat').evaluate((el) => el.clientWidth);
/** A pseudo-element's computed style. */
const after = (loc, prop) => loc.evaluate((el, p) => getComputedStyle(el, '::after')[p], prop);

async function theme(yap, main, which) {
  await setConfig(yap, main, { theme: which });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe(which);
}

/** This window's own layout choices forgotten: the rail expanded, the list shown at 224, the split at its default. */
async function forgetLayout(main) {
  await main.evaluate((key) => {
    for (const k of [key, 'yapChatListHidden', 'yapChatListWidth', 'yapChatDocWidth']) localStorage.removeItem(k);
  }, RAIL_KEY);
}

/** Reloaded, the window's storage read afresh (Home is where it opens). */
async function reload(main) {
  await main.reload({ waitUntil: 'load' });
  await expect(rail(main)).toBeVisible();
}

async function send(main, text) {
  await box(main).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
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

/** A new chat whose answer writes document `title` beside it (its page editable once written). */
async function openDocument(main, fakeLlm, title) {
  fakeLlm.scriptChat([
    { tools: [{ name: 'create_document', args: { title, sections: [{ key: 'Beds', intent: 'what goes where' }] } }] },
    { tools: [{ name: 'write_section', args: (r) => ({ note_id: madeDoc(r), key: 'Beds', markdown: 'Beans by the fence, peas by the shed.' }) }] },
    { content: "It's open on the right." },
  ]);
  await openView(main, 'Chat');
  if (await showBtn(main).isVisible()) await showBtn(main).click();
  await main.getByRole('button', { name: 'New chat' }).click();
  await send(main, `Plan my ${title.toLowerCase()}`);
  await expect(msgs(main)).toContainText("It's open on the right.", { timeout: 20_000 });
  await expect(editor(main)).toHaveAttribute('contenteditable', 'true');
}

/** A long answer (never the same line twice: an answer repeating itself is cut, chat/stream.rs). */
const LONG = Array.from({ length: 36 }, (_, i) => `Bed ${i + 1} gets ${['beans', 'peas', 'kale', 'leeks', 'chard', 'garlic'][i % 6]} and ${(i % 7) + 1} litres of water a week, sown in week ${((i * 3) % 13) + 1} once the soil is warm enough to work.`).join('\n\n');

/** A conversation long enough to scroll, its last answer in. */
async function longChat(main, fakeLlm) {
  fakeLlm.scriptChat([{ content: LONG }]);
  await openView(main, 'Chat');
  if (await showBtn(main).isVisible()) await showBtn(main).click();
  await main.getByRole('button', { name: 'New chat' }).click();
  await send(main, 'What goes in each of the garden beds this year, and how much water does each one need?');
  await expect(main.locator('.cbubble.assistant').last()).toContainText('Bed 36 gets');
  await expect.poll(() => msgs(main).evaluate((m) => m.scrollHeight > m.clientHeight + 4)).toBe(true);
}

/**
 * Nothing overlapping, nothing cut off: the rail, the view, Chat's list,
 * thread and panel side by side inside the window; the rail's rows inside
 * it; the composer and its chips inside the thread; no sideways scroll.
 */
async function expectNothingOverlaps(main, where) {
  // (The rail where it's going: not mid-glide after a ☰ just pressed.)
  await expect
    .poll(() => rail(main).evaluate((el) => !el.classList.contains('glide') && [56, 220].includes(Math.round(el.getBoundingClientRect().width))), { message: `${where}: the rail settled` })
    .toBe(true);
  const bad = await main.evaluate(() => {
    const out = [];
    const r = (el) => el?.getBoundingClientRect();
    const shown = (el) => el && el.getClientRects().length && getComputedStyle(el).visibility !== 'hidden';
    const W = window.innerWidth;
    if (document.documentElement.scrollWidth > W + 0.5) out.push(`the page scrolls sideways (${document.documentElement.scrollWidth} > ${W})`);
    const side = r(document.querySelector('[data-nav-rail]'));
    const view = r(document.querySelector('main.view'));
    if (side.right > view.left + 0.5) out.push('the rail under the view');
    if (view.right > W + 0.5) out.push('the view past the window');
    for (const b of document.querySelectorAll('[data-nav-rail] button')) {
      if (!shown(b)) continue;
      const x = r(b);
      if (x.left < side.left - 0.5 || x.right > side.right + 0.5) out.push(`${b.getAttribute('aria-label') || b.textContent.trim()} outside the rail`);
    }
    for (const b of document.querySelectorAll('.titlebar button')) {
      if (shown(b) && r(b).right > W + 0.5) out.push(`${b.getAttribute('aria-label')} past the window`);
    }
    const chat = document.querySelector('.chat');
    if (chat) {
      const list = document.querySelector('.listslot:not(.aside):not(.floating) .convs');
      const thread = r(document.querySelector('section.thread'));
      const slot = document.querySelector('[data-testid="chat-panel-slot"]:not(.cover)');
      if (list && r(list).right > thread.left + 0.5) out.push('the list over the thread');
      if (slot && thread.right > r(slot).left + 0.5) out.push('the thread under the panel');
      if (slot && r(slot).right > view.right + 0.5) out.push('the panel past the view');
      if (thread.width < 339.5 && !document.querySelector('[data-testid="chat-panel-slot"].cover')) out.push(`the thread ${thread.width} px`);
      // The composer's parts inside the thread (its 20 px padding kept), its chips side by side.
      const comp = document.querySelector('.composer');
      if (comp) {
        for (const e of comp.querySelectorAll('button, .inputbar')) {
          if (shown(e) && r(e).right > thread.right - 20 + 0.5) out.push(`${e.getAttribute('aria-label') || e.className} cut off`);
        }
        const chips = [...document.querySelectorAll('.composerfoot button')].filter(shown).map((e) => ({ name: e.getAttribute('aria-label') || e.textContent.trim(), b: r(e) })).sort((a, b) => a.b.left - b.b.left);
        for (let i = 1; i < chips.length; i++) if (chips[i].b.left < chips[i - 1].b.right - 0.5) out.push(`${chips[i].name} over ${chips[i - 1].name}`);
      }
    }
    return out;
  });
  expect(bad, where).toEqual([]);
}

test.beforeEach(async ({ yap, main, fakeLlm }) => {
  await main.setViewportSize({ width: 1280, height: 800 });
  await forgetLayout(main);
  await reload(main);
  await setConfig(yap, main, {
    theme: 'light',
    motion: 'system',
    agentsEnabled: false,
    webSearch: false,
    speakReplies: false,
    llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat', apiKey: '', apiKeys: {}, prompt: '', disableThinking: false } },
  });
});

test.afterEach(async ({ main }) => {
  await main.setViewportSize({ width: 1200, height: 800 });
  await forgetLayout(main);
});

// ---------------------------------------------------------------- no app

test('navRail.js and chatRoom.js: the rail’s toggle and key, what gives way first, the list’s width and edge', () => {
  // The toggle: narrow → expanded (kept open over a view that narrowed it); expanded → the person's collapse.
  expect(railToggle({ narrow: true, auto: false })).toEqual({ collapsed: false, keptOpen: false });
  expect(railToggle({ narrow: true, auto: true })).toEqual({ collapsed: false, keptOpen: true });
  expect(railToggle({ narrow: false, auto: true })).toEqual({ collapsed: true, keptOpen: false });
  // Ctrl+\ by its character, its place, or a page's key code; never with Shift or Alt, nor plain.
  expect(isRailKey({ ctrlKey: true, key: '\\', code: 'Backslash' })).toBe(true);
  expect(isRailKey({ ctrlKey: true, key: '#', code: 'Backslash' })).toBe(true);
  expect(isRailKey({ ctrlKey: true, keyCode: 220 })).toBe(true);
  expect(isRailKey({ ctrlKey: true, shiftKey: true, key: '|', code: 'Backslash' })).toBe(false);
  expect(isRailKey({ ctrlKey: true, altKey: true, key: '\\' })).toBe(false);
  expect(isRailKey({ key: '\\', code: 'Backslash' })).toBe(false);
  expect(isRailKey({ ctrlKey: true, key: 'b' })).toBe(false);

  // A window 1,400 px wide: Chat is 1,166 beside the rail expanded, 1,330 beside its icons.
  const at1400 = { chat: 1400 - RAIL_WIDE - FRAME, rail: RAIL_WIDE };
  // No panel: nothing gives way.
  expect(chatRoom(at1400)).toMatchObject({ railAuto: false, railNarrow: false, listShown: true, width: 1166 });
  // A panel: 1. the rail narrows — and that's enough for the list to stay.
  expect(chatRoom({ ...at1400, panelOpen: true })).toMatchObject({ railAuto: true, railNarrow: true, width: 1330, listShown: true, listAuto: false, panelWidth: DEFAULT_WIDTH });
  // 2. At 1,280 even its icons leave Chat under LIST_STAYS: the list steps aside too.
  const at1280 = { chat: 1280 - RAIL_WIDE - FRAME, rail: RAIL_WIDE };
  expect(chatRoom({ ...at1280, panelOpen: true })).toMatchObject({ railAuto: true, width: 1210, listShown: false, listAuto: true, panelWidth: DEFAULT_WIDTH });
  // Wide enough beside the rail expanded (1,474 and up): nothing gives way.
  const at1520 = { chat: 1520 - RAIL_WIDE - FRAME, rail: RAIL_WIDE };
  expect(chatRoom({ ...at1520, panelOpen: true })).toMatchObject({ railAuto: false, railNarrow: false, width: 1286, listShown: true, panelWidth: DEFAULT_WIDTH });
  expect(chatRoom({ chat: 1473 - RAIL_WIDE - FRAME, rail: RAIL_WIDE, panelOpen: true }).railAuto).toBe(true);
  expect(chatRoom({ chat: 1474 - RAIL_WIDE - FRAME, rail: RAIL_WIDE, panelOpen: true }).railAuto).toBe(false);
  // The list hidden by the person: the rail needs less (the thread and the panel).
  expect(LIST_STAYS - LIST_WIDTH).toBe(1016);
  expect(chatRoom({ chat: 1300 - RAIL_WIDE - FRAME, rail: RAIL_WIDE, panelOpen: true, hidden: true })).toMatchObject({ railAuto: false });
  expect(chatRoom({ chat: 1200 - RAIL_WIDE - FRAME, rail: RAIL_WIDE, panelOpen: true, hidden: true })).toMatchObject({ railAuto: true });
  // The person's own: collapsed stays narrow (not Yap's); kept open stays expanded, the list going instead.
  expect(chatRoom({ ...at1400, panelOpen: true, railCollapsed: true })).toMatchObject({ railAuto: false, railNarrow: true, width: 1330 });
  expect(chatRoom({ ...at1400, panelOpen: true, railKeptOpen: true })).toMatchObject({ railAuto: false, railNarrow: false, width: 1166, listShown: false });
  // Gliding (measured mid-way, the two adding up the same): the same decisions, the panel held to the room there is now.
  const mid = chatRoom({ chat: 1400 - 140 - FRAME, rail: 140, gliding: true, panelOpen: true, want: 900 });
  expect(mid).toMatchObject({ railNarrow: true, width: 1330, listShown: true, panelWidth: 1400 - 140 - FRAME - LIST_WIDTH - THREAD_MIN });
  expect(chatRoom({ chat: 1400 - 56 - FRAME, rail: 56, panelOpen: true, want: 900 }).panelWidth).toBe(1330 - LIST_WIDTH - THREAD_MIN);
  // Yap's own step (no glide), measured before it lands: the room it's about to have, at once.
  expect(chatRoom({ chat: 1400 - RAIL_WIDE - FRAME, rail: RAIL_WIDE, panelOpen: true, want: 900 }).panelWidth).toBe(1330 - LIST_WIDTH - THREAD_MIN);
  // 3. the panel's width, then 4. it covers: at 860 (the window's least), 700.
  expect(chatRoom({ chat: 860 - RAIL_WIDE - FRAME, rail: RAIL_WIDE, panelOpen: true })).toMatchObject({ railNarrow: true, width: 790, listShown: false, panelWidth: 790 - THREAD_MIN, cover: false });
  expect(chatRoom({ chat: 700 - RAIL_WIDE - FRAME, rail: RAIL_WIDE, panelOpen: true })).toMatchObject({ railNarrow: true, cover: true });
  // No rail measured (0): as before it.
  expect(chatRoom({ chat: 966, panelOpen: true })).toMatchObject({ railAuto: false, railNarrow: false, width: 966, listShown: false });

  // The list's width: the person's, held to 200–420 and to the thread's least.
  expect(chatRoom({ chat: 1200, list: 300 })).toMatchObject({ listWidth: 300, listMax: LIST_MAX });
  expect(chatRoom({ chat: 1200, list: 999 })).toMatchObject({ listWidth: LIST_MAX });
  expect(chatRoom({ chat: 1200, list: 50 })).toMatchObject({ listWidth: LIST_MIN });
  expect(chatRoom({ chat: 600, list: 400 })).toMatchObject({ listWidth: 600 - THREAD_MIN });
  // Beside a panel the edge stops where the panel keeps its least (never floating mid-drag).
  const beside = chatRoom({ chat: 900, panelOpen: true, shownForPanel: true, list: 220 });
  expect(beside).toMatchObject({ listShown: true, listFloats: false, listWidth: 220, listMax: 900 - THREAD_MIN - MIN_WIDTH, panelWidth: 900 - 220 - THREAD_MIN });
  expect(chatRoom({ chat: 1300, panelOpen: true, shownForPanel: true, list: 300 })).toMatchObject({ listWidth: 300, listMax: LIST_MAX, panelWidth: DEFAULT_WIDTH });
  // Not measured yet: its own width.
  expect(chatRoom({ list: 320 })).toMatchObject({ listWidth: 320, listMax: LIST_MAX });
  // Its keys: → wider, ← narrower (Shift: further), Home / End; its drag, held.
  expect(listKeyWidth('ArrowRight', { width: 224 })).toBe(248);
  expect(listKeyWidth('ArrowLeft', { width: 224, shift: true })).toBe(LIST_MIN);
  expect(listKeyWidth('ArrowRight', { width: 400, shift: true })).toBe(LIST_MAX);
  expect(listKeyWidth('Home', { width: 300 })).toBe(LIST_MIN);
  expect(listKeyWidth('End', { width: 300 })).toBe(LIST_MAX);
  expect(listKeyWidth('End', { width: 300, maxWidth: 350 })).toBe(350);
  expect(listKeyWidth('Enter', { width: 300 })).toBeNull();
  expect(listDragWidth(400, { left: 100 })).toBe(300);
  expect(listDragWidth(1000, { left: 100 })).toBe(LIST_MAX);
  expect(listDragWidth(150, { left: 100 })).toBe(LIST_MIN);
  expect(listDragWidth(500, { left: 100, maxWidth: 330 })).toBe(330);
});

// ---------------------------------------------------------------- the rail

test('the rail: ☰ and Ctrl+\\ collapse and expand it in every view, gliding, remembered; tooltips and badges', async ({ yap, main, shot }) => {
  await yap.invoke('e2e_guide_reset');
  await reload(main);
  await openView(main, 'Home');
  // Expanded on a first run: 220 px, its labels, ☰ at its top.
  expect(await railWidth(main)).toBe(RAIL_WIDE);
  await expect(railBtn(main)).toHaveAccessibleName('Collapse the sidebar');
  await expect(railBtn(main)).toHaveAttribute('aria-expanded', 'true');
  await expect(railBtn(main)).toHaveAttribute('aria-keyshortcuts', 'Control+\\');
  await expect(railBtn(main)).toHaveAttribute('data-tip', 'Collapse the sidebar · Ctrl+\\');
  expect((await rect(railBtn(main))).y).toBeLessThan((await rect(navItem(main, 'Home'))).y);
  await expect(main.getByTestId('meeting-setup')).toBeVisible();
  const homeIcon = await rect(navItem(main, 'Home').locator('.navicon'));
  expect(homeIcon.x + homeIcon.w / 2).toBe(RAIL_NARROW / 2);
  const viewBefore = await rect(main.locator('main.view'));
  // A label shown, no tooltip on a row while expanded (☰ has its own).
  await navItem(main, 'Notes').hover();
  await main.waitForTimeout(400);
  expect(await after(navItem(main, 'Notes'), 'opacity')).toBe('0');
  await shot(main, '01-rail-expanded-light');
  await theme(yap, main, 'dark');
  await shot(main, '02-rail-expanded-dark');
  await theme(yap, main, 'light');

  // ☰: one short glide to 56 px, every icon where it was, the view wider by the difference.
  await main.evaluate(() => {
    const side = document.querySelector('[data-nav-rail]');
    const icon = side.querySelector('.nav .navicon');
    const rec = { frames: [], on: true };
    window.__rail = rec;
    const step = () => {
      rec.frames.push({ w: Math.round(side.getBoundingClientRect().width * 10) / 10, icon: Math.round(icon.getBoundingClientRect().x) });
      if (rec.on) requestAnimationFrame(step);
    };
    requestAnimationFrame(step);
  });
  await railBtn(main).click();
  await expect.poll(() => railWidth(main)).toBe(RAIL_NARROW);
  await main.waitForTimeout(150);
  const frames = await main.evaluate(() => {
    window.__rail.on = false;
    return window.__rail.frames;
  });
  const widths = frames.map((f) => f.w);
  expect(widths.at(-1)).toBe(RAIL_NARROW);
  for (let i = 1; i < widths.length; i++) expect(widths[i], 'never wider again on the way').toBeLessThanOrEqual(widths[i - 1]);
  const between = widths.filter((w) => w < RAIL_WIDE && w > RAIL_NARROW);
  expect(between.length, 'it glides').toBeGreaterThan(1);
  expect(new Set(frames.map((f) => f.icon)).size, 'the icons never move').toBe(1);
  await expect(railBtn(main)).toHaveAccessibleName('Expand the sidebar');
  await expect(railBtn(main)).toHaveAttribute('aria-expanded', 'false');
  await expect(railBtn(main)).toHaveAttribute('data-tip', 'Expand the sidebar · Ctrl+\\');
  expect(await main.evaluate((k) => localStorage.getItem(k), RAIL_KEY)).toBe('1');
  const viewNarrow = await rect(main.locator('main.view'));
  expect(viewNarrow.w).toBe(viewBefore.w + RAIL_WIDE - RAIL_NARROW);
  expect(await rect(navItem(main, 'Home').locator('.navicon'))).toEqual(homeIcon);
  // Every row still named (its label is the button's name and its tooltip).
  for (const name of ['Home', 'Insights', 'Chat', 'Notes', 'Meetings', 'Tasks', 'Upload', 'Dictionary', 'Integrations', 'Settings']) {
    await expect(navItem(main, name)).toBeVisible();
    expect((await rect(navItem(main, name))).w).toBe(RAIL_NARROW - 16);
  }

  // Tooltips: an icon's name on a hover (after a beat) and on the keyboard's focus; ☰'s with its key.
  const notes = navItem(main, 'Notes');
  await notes.hover();
  await expect.poll(() => after(notes, 'opacity')).toBe('1');
  // (Drawn with an empty alt text: never part of the button's name, which stays "Notes".)
  expect(await after(notes, 'content')).toBe('"Notes" / ""');
  await expect(notes).toHaveAccessibleName('Notes');
  await shot(main, '03-rail-collapsed-tooltip-light');
  await railBtn(main).hover();
  await expect.poll(() => after(railBtn(main), 'opacity')).toBe('1');
  expect(await after(railBtn(main), 'content')).toBe('"Expand the sidebar · Ctrl+\\\\" / ""');
  await main.mouse.move(700, 400);
  await expect.poll(() => after(notes, 'opacity')).toBe('0');
  await navItem(main, 'Upload').focus();
  await main.keyboard.press('Shift+Tab');
  await main.keyboard.press('Tab');
  await expect(navItem(main, 'Upload')).toBeFocused();
  await expect.poll(() => after(navItem(main, 'Upload'), 'opacity')).toBe('1');
  // A hover never opens it, however long.
  await navItem(main, 'Chat').hover();
  await main.waitForTimeout(800);
  expect(await railWidth(main)).toBe(RAIL_NARROW);

  // Badges: the bell's count (the stub build has no speech model), the meeting setup's what's-left.
  const bell = main.getByRole('button', { name: /^Notifications/ });
  const count = await bell.locator('.belldot').textContent();
  expect(Number(count)).toBeGreaterThanOrEqual(1);
  await expect(bell.locator('.belldot')).toBeVisible();
  const mini = main.getByTestId('meeting-setup-mini');
  await expect(mini).toBeVisible();
  await expect(mini).toHaveAccessibleName('Set up meeting notes, 0 of 3 done');
  await expect(mini.locator('.left')).toHaveText('3');
  await expect(main.getByTestId('meeting-setup')).toHaveCount(0);
  await main.mouse.move(700, 400);
  await main.evaluate(() => document.activeElement?.blur());
  await expect.poll(() => after(navItem(main, 'Upload'), 'opacity')).toBe('0');
  await shot(main, '04-rail-collapsed-light');
  await theme(yap, main, 'dark');
  await shot(main, '05-rail-collapsed-dark');
  await theme(yap, main, 'light');
  // The account and Settings at its foot, as icons inside it.
  for (const loc of [navItem(main, 'Settings'), sidebar(main).locator('.acct')]) {
    const r = await rect(loc);
    expect(r.right).toBeLessThanOrEqual(RAIL_NARROW - 8);
  }
  await expect(sidebar(main).locator('.acct')).toHaveAttribute('data-tip', 'Sign in');

  // Reloaded: still narrow (Home). The setup's icon expands it, the card there.
  await reload(main);
  expect(await railWidth(main)).toBe(RAIL_NARROW);
  await mini.click();
  await expect.poll(() => railWidth(main)).toBe(RAIL_WIDE);
  await expect(main.getByTestId('meeting-setup')).toBeVisible();
  expect(await main.evaluate((k) => localStorage.getItem(k), RAIL_KEY)).toBeNull();

  // Ctrl+\ in another view (Notes): collapsed, and again expanded; remembered there too.
  await openView(main, 'Notes');
  await main.keyboard.press('Control+Backslash');
  await expect.poll(() => railWidth(main)).toBe(RAIL_NARROW);
  await reload(main);
  await openView(main, 'Notes');
  expect(await railWidth(main)).toBe(RAIL_NARROW);
  await main.keyboard.press('Control+Backslash');
  await expect.poll(() => railWidth(main)).toBe(RAIL_WIDE);
  await reload(main);
  await openView(main, 'Notes');
  expect(await railWidth(main)).toBe(RAIL_WIDE);
  // Enter on ☰ works as a click.
  await railBtn(main).focus();
  await main.keyboard.press('Enter');
  await expect.poll(() => railWidth(main)).toBe(RAIL_NARROW);
  await main.keyboard.press('Enter');
  await expect.poll(() => railWidth(main)).toBe(RAIL_WIDE);

  // Not from Settings (it's over everything); listed on the shortcut sheet.
  await navItem(main, 'Settings').click();
  await expect(settingsDialog(main)).toBeVisible();
  await main.keyboard.press('Control+Backslash');
  await main.waitForTimeout(300);
  expect(await railWidth(main)).toBe(RAIL_WIDE);
  await closeSettings(main);
  await main.keyboard.press('Control+/');
  const sheet = main.getByRole('dialog', { name: 'Keyboard shortcuts' });
  await expect(sheet.getByRole('region', { name: "In Yap's window" })).toContainText('Collapse or expand the sidebar');
  await main.keyboard.press('Control+Backslash');
  await main.waitForTimeout(300);
  expect(await railWidth(main)).toBe(RAIL_WIDE);
  await main.keyboard.press('Escape');
  await expect(sheet).toHaveCount(0);

  // Motion reduced: no glide, the same narrow rail at once.
  await setConfig(yap, main, { motion: 'reduced' });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.motion)).toBe('reduced');
  await railBtn(main).click();
  expect(await railWidth(main)).toBe(RAIL_NARROW);
  expect(await rail(main).evaluate((el) => getComputedStyle(el).transitionDuration)).toBe('0s');
  await railBtn(main).click();
  expect(await railWidth(main)).toBe(RAIL_WIDE);
});

// ---------------------------------------------------------------- Chat gives way

/** Each frame from now on: the rail's width, whether it glides, the list, the thread, the panel (`window.__room`). */
function recordRoom() {
  const q = (s) => document.querySelector(s);
  const w = (el) => (el && el.getClientRects().length ? Math.round(el.getBoundingClientRect().width) : null);
  const rec = { frames: [], on: true };
  window.__room = rec;
  const step = () => {
    const side = q('[data-nav-rail]');
    rec.frames.push({ rail: w(side), glide: side.classList.contains('glide'), list: w(q('.listslot:not(.aside) .convs')), thread: w(q('section.thread')), slot: w(q('[data-testid="chat-panel-slot"]')) });
    if (rec.on) requestAnimationFrame(step);
  };
  requestAnimationFrame(step);
}

test('Chat gives way in order as the window narrows with a document open: the rail, the list, the panel’s width, then the panel covers', async ({ yap, main, fakeLlm, shot }) => {
  await main.setViewportSize({ width: 1600, height: 820 });
  // Wide: a document beside the list and the thread, the rail as it was.
  await openDocument(main, fakeLlm, 'Herb spiral');
  expect(await railWidth(main)).toBe(RAIL_WIDE);
  await expect(list(main)).toBeVisible();
  expect((await rect(slot(main))).w).toBe(DEFAULT_WIDTH);
  await panel(main).getByRole('button', { name: 'Close the document' }).click();

  // 1,400: the document opens and, in that frame, the rail narrows (nothing gliding); the list stays.
  await main.setViewportSize({ width: 1400, height: 820 });
  await main.evaluate(recordRoom);
  await main.locator('[data-testid="chat-doc-pill"]').filter({ hasText: 'Herb spiral' }).click();
  await expect(panel(main)).toBeVisible();
  await main.waitForTimeout(400);
  const frames = await main.evaluate(() => {
    window.__room.on = false;
    return window.__room.frames;
  });
  expect(frames.some((f) => f.glide), 'Yap’s own step never glides').toBe(false);
  expect(new Set(frames.map((f) => f.rail))).toEqual(new Set([RAIL_WIDE, RAIL_NARROW]));
  const opened = frames.findIndex((f) => f.slot);
  expect(frames[opened].rail, 'the rail narrow in the panel’s first frame').toBe(RAIL_NARROW);
  expect(frames[opened - 1].rail).toBe(RAIL_WIDE);
  expect(frames[opened].slot).toBe(DEFAULT_WIDTH);
  expect(new Set(frames.slice(opened).map((f) => f.thread)).size, 'the thread changed once').toBe(1);
  await expect(list(main)).toBeVisible();
  await expect(railBtn(main)).toHaveAccessibleName('Expand the sidebar');
  // Yap's, never remembered.
  expect(await main.evaluate((k) => localStorage.getItem(k), RAIL_KEY)).toBeNull();
  await shot(main, '06-give-way-1-the-rail');

  // 1,280: then the list.
  await main.setViewportSize({ width: 1280, height: 820 });
  await expect(list(main)).toBeHidden();
  await expect(showBtn(main)).toBeVisible();
  expect(await railWidth(main)).toBe(RAIL_NARROW);
  await expect.poll(async () => (await rect(slot(main))).w).toBe(DEFAULT_WIDTH);
  expect(await chatWidth(main)).toBe(1280 - RAIL_NARROW - FRAME);
  await shot(main, '07-give-way-2-the-list');

  // 860 (the window's least): then the panel's width, the thread keeping its least.
  await main.setViewportSize({ width: 860, height: 700 });
  await expect.poll(async () => (await rect(slot(main))).w).toBe(860 - RAIL_NARROW - FRAME - THREAD_MIN);
  expect((await rect(threadBox(main))).w).toBe(THREAD_MIN);
  await expectNothingOverlaps(main, 'at 860 with the document open');
  await shot(main, '08-give-way-3-the-panel');

  // 700: the panel covers the conversation.
  await main.setViewportSize({ width: 700, height: 640 });
  await expect(slot(main)).toHaveClass(/\bcover\b/);
  await shot(main, '09-give-way-4-the-panel-covers');

  // Wider again: each comes back in the opposite order.
  await main.setViewportSize({ width: 1280, height: 820 });
  await expect(slot(main)).not.toHaveClass(/\bcover\b/);
  await expect(list(main)).toBeHidden();
  await main.setViewportSize({ width: 1400, height: 820 });
  await expect(list(main)).toBeVisible();
  expect(await railWidth(main)).toBe(RAIL_NARROW);
  await main.setViewportSize({ width: 1600, height: 820 });
  await expect.poll(() => railWidth(main)).toBe(RAIL_WIDE);
  await expect(list(main)).toBeVisible();

  // The person's ☰ wins for that panel: at 1,400 expanded again, the list goes instead.
  await main.setViewportSize({ width: 1400, height: 820 });
  await expect.poll(() => railWidth(main)).toBe(RAIL_NARROW);
  await railBtn(main).click();
  await expect.poll(() => railWidth(main)).toBe(RAIL_WIDE);
  await expect(list(main)).toBeHidden();
  expect(await main.evaluate((k) => localStorage.getItem(k), RAIL_KEY)).toBeNull();
  await expect.poll(async () => (await rect(threadBox(main))).w).toBeGreaterThanOrEqual(THREAD_MIN);
  // The panel closed: the rail as the person left it (expanded), the list back.
  await panel(main).getByRole('button', { name: 'Close the document' }).click();
  await expect(list(main)).toBeVisible();
  expect(await railWidth(main)).toBe(RAIL_WIDE);
  // Opened again: narrowed again for it (kept open for that panel only).
  await main.locator('[data-testid="chat-doc-pill"]').filter({ hasText: 'Herb spiral' }).click();
  await expect.poll(() => railWidth(main)).toBe(RAIL_NARROW);
  // Collapsed by the person: theirs, remembered — it stays narrow once the panel closes.
  await railBtn(main).click();
  await expect.poll(() => railWidth(main)).toBe(RAIL_WIDE);
  await railBtn(main).click();
  await expect.poll(() => railWidth(main)).toBe(RAIL_NARROW);
  expect(await main.evaluate((k) => localStorage.getItem(k), RAIL_KEY)).toBe('1');
  await panel(main).getByRole('button', { name: 'Close the document' }).click();
  await main.waitForTimeout(300);
  expect(await railWidth(main)).toBe(RAIL_NARROW);
  // Expanded by them over a panel that wants the room: it stays expanded (Yap never takes it back).
  await main.locator('[data-testid="chat-doc-pill"]').filter({ hasText: 'Herb spiral' }).click();
  await expect(panel(main)).toBeVisible();
  await railBtn(main).click();
  await expect.poll(() => railWidth(main)).toBe(RAIL_WIDE);
  await main.waitForTimeout(400);
  expect(await railWidth(main)).toBe(RAIL_WIDE);
  expect(await main.evaluate((k) => localStorage.getItem(k), RAIL_KEY)).toBeNull();
  await panel(main).getByRole('button', { name: 'Close the document' }).click();
  // Leaving Chat with a panel narrowing it gives it back.
  expect(await railWidth(main)).toBe(RAIL_WIDE);
  await main.locator('[data-testid="chat-doc-pill"]').filter({ hasText: 'Herb spiral' }).click();
  await expect.poll(() => railWidth(main)).toBe(RAIL_NARROW);
  await openView(main, 'Home');
  await expect.poll(() => railWidth(main)).toBe(RAIL_WIDE);
});

// ---------------------------------------------------------------- the list's edge

test('the list’s edge: dragged, Esc, held to 200–420, a double-click back, its keys, remembered; Ctrl+B hides it whole', async ({ yap, main, shot }) => {
  await openView(main, 'Chat');
  await expect(list(main)).toBeVisible();
  const e = edge(main);
  await expect(e).toHaveAttribute('aria-valuenow', String(LIST_WIDTH));
  await expect(e).toHaveAttribute('aria-valuemin', String(LIST_MIN));
  await expect(e).toHaveAttribute('aria-valuemax', String(LIST_MAX));
  await expect(e).toHaveAttribute('title', 'Drag to resize · Double-click to reset');
  // Over the list's right edge, its resize cursor.
  const l = await rect(list(main));
  const h = await rect(e);
  expect(Math.abs(h.x + h.w / 2 - l.right)).toBeLessThanOrEqual(1);
  expect(await e.evaluate((el) => getComputedStyle(el).cursor)).toBe('col-resize');

  // A drag 100 px right: 324; the thread gives the room.
  const thread0 = await rect(threadBox(main));
  const cy = h.y + h.h / 2;
  let cx = h.x + h.w / 2;
  await main.mouse.move(cx, cy);
  await main.mouse.down();
  await main.mouse.move(cx + 60, cy, { steps: 4 });
  await main.mouse.move(cx + 100, cy, { steps: 4 });
  await expect(main.locator('.chat')).toHaveClass(/\bresizing\b/);
  await expect.poll(async () => (await rect(list(main))).w).toBe(LIST_WIDTH + 100);
  await shot(main, '10-list-edge-mid-drag-light');
  await theme(yap, main, 'dark');
  await shot(main, '11-list-edge-mid-drag-dark');
  await theme(yap, main, 'light');
  await main.mouse.up();
  await expect(e).toHaveAttribute('aria-valuenow', String(LIST_WIDTH + 100));
  expect((await rect(threadBox(main))).w).toBe(thread0.w - 100);
  expect(await main.evaluate(() => localStorage.getItem('yapChatListWidth'))).toBe(String(LIST_WIDTH + 100));

  // Esc mid-drag: back where it was.
  cx = (await rect(e)).x + 5;
  await main.mouse.move(cx, cy);
  await main.mouse.down();
  await main.mouse.move(cx - 80, cy, { steps: 4 });
  await expect.poll(async () => (await rect(list(main))).w).toBe(LIST_WIDTH + 20);
  await main.keyboard.press('Escape');
  await main.mouse.up();
  await expect.poll(async () => (await rect(list(main))).w).toBe(LIST_WIDTH + 100);

  // Held: far right 420, far left 200.
  cx = (await rect(e)).x + 5;
  await main.mouse.move(cx, cy);
  await main.mouse.down();
  await main.mouse.move(cx + 600, cy, { steps: 6 });
  await main.mouse.up();
  await expect.poll(async () => (await rect(list(main))).w).toBe(LIST_MAX);
  cx = (await rect(e)).x + 5;
  await main.mouse.move(cx, cy);
  await main.mouse.down();
  await main.mouse.move(cx - 600, cy, { steps: 6 });
  await main.mouse.up();
  await expect.poll(async () => (await rect(list(main))).w).toBe(LIST_MIN);
  // A double-click: 224 again.
  await e.dblclick();
  await expect(e).toHaveAttribute('aria-valuenow', String(LIST_WIDTH));
  expect(await main.evaluate(() => localStorage.getItem('yapChatListWidth'))).toBeNull();

  // Its keys: → wider (Shift: further), ← narrower, Home / End.
  await e.focus();
  await main.keyboard.press('ArrowRight');
  await expect(e).toHaveAttribute('aria-valuenow', String(LIST_WIDTH + 24));
  await main.keyboard.press('Shift+ArrowRight');
  await expect(e).toHaveAttribute('aria-valuenow', String(LIST_WIDTH + 104));
  await main.keyboard.press('ArrowLeft');
  await expect(e).toHaveAttribute('aria-valuenow', String(LIST_WIDTH + 80));
  await main.keyboard.press('Home');
  await expect(e).toHaveAttribute('aria-valuenow', String(LIST_MIN));
  await main.keyboard.press('End');
  await expect(e).toHaveAttribute('aria-valuenow', String(LIST_MAX));
  await main.keyboard.press('Shift+ArrowLeft');
  await expect(e).toHaveAttribute('aria-valuenow', String(LIST_MAX - 80));
  expect((await rect(list(main))).w).toBe(LIST_MAX - 80);

  // Remembered: a reload, Chat again.
  await reload(main);
  await openView(main, 'Chat');
  expect((await rect(list(main))).w).toBe(LIST_MAX - 80);
  // Ctrl+B hides it whole and brings it back at its width; the rail's ☰ is another thing.
  await box(main).click();
  await main.keyboard.press('Control+b');
  await expect(list(main)).toBeHidden();
  await expect(edge(main)).toBeHidden();
  await main.keyboard.press('Control+b');
  await expect(list(main)).toBeVisible();
  expect((await rect(list(main))).w).toBe(LIST_MAX - 80);

  // A narrow window: the list gives way so the thread keeps its least.
  await e.focus();
  await main.keyboard.press('End');
  await main.setViewportSize({ width: 900, height: 760 });
  const width = await chatWidth(main);
  await expect.poll(async () => (await rect(list(main))).w).toBe(width - THREAD_MIN);
  expect((await rect(threadBox(main))).w).toBe(THREAD_MIN);
  await expectNothingOverlaps(main, 'a wide list in a 900 px window');
  await shot(main, '12-list-wide-in-a-narrow-window');
});

// ---------------------------------------------------------------- the readable column

/** The column's and the composer's boxes, and the thread's. */
async function columnBoxes(main) {
  return { col: await rect(column(main)), comp: await rect(composer(main)), thread: await rect(threadBox(main)), msgs: await msgs(main).evaluate((m) => ({ client: m.clientWidth, x: Math.round(m.getBoundingClientRect().x) })) };
}

test('a wide thread reads in one centred 768 px column, the composer on it; a narrow thread is its own width', async ({ yap, main, fakeLlm, shot }) => {
  await main.setViewportSize({ width: 1920, height: 1040 });
  await longChat(main, fakeLlm);
  // Everything collapsed: the rail (☰) and the list (Ctrl+B).
  await railBtn(main).click();
  await expect.poll(() => railWidth(main)).toBe(RAIL_NARROW);
  await box(main).click();
  await main.keyboard.press('Control+b');
  await expect(list(main)).toBeHidden();
  await expect.poll(async () => (await rect(threadBox(main))).w).toBe(1920 - RAIL_NARROW - FRAME);
  let b = await columnBoxes(main);
  expect(b.col.w).toBe(COLUMN);
  expect(b.comp.w).toBe(COLUMN);
  // One centre, the scrollbar's half allowed for; the same left and right edges.
  expect(Math.abs(b.col.x - b.comp.x)).toBeLessThanOrEqual(1);
  expect(Math.abs(b.col.right - b.comp.right)).toBeLessThanOrEqual(1);
  // Centred on the thread's scroller (its scrollbar at the thread's edge).
  // (Its client box is its 2 px padding either side of the content, the scrollbar beyond.)
  expect(Math.abs(b.col.x - (b.msgs.x + (b.msgs.client - COLUMN) / 2))).toBeLessThanOrEqual(1);
  // Every bubble, the answer's line and the code label within it.
  const out = await main.evaluate(() => {
    const c = document.querySelector('[data-testid="chat-column"]').getBoundingClientRect();
    return [...document.querySelectorAll('.cbubble, .msgsin > *')].filter((e) => {
      const r = e.getBoundingClientRect();
      return r.width && (r.left < c.left - 0.5 || r.right > c.right + 0.5);
    }).length;
  });
  expect(out, 'nothing outside the column').toBe(0);
  await shot(main, '13-chat-1920-everything-collapsed-light');
  await theme(yap, main, 'dark');
  await shot(main, '14-chat-1920-everything-collapsed-dark');
  await theme(yap, main, 'light');
  await expectNothingOverlaps(main, '1920, everything collapsed');

  // The list shown (the column moves with the thread's centre, never off it); the rail expanded.
  await main.keyboard.press('Control+b');
  await expect(list(main)).toBeVisible();
  await railBtn(main).click();
  await expect.poll(() => railWidth(main)).toBe(RAIL_WIDE);
  b = await columnBoxes(main);
  expect(b.col.w).toBe(COLUMN);
  expect(Math.abs(b.col.x - b.comp.x)).toBeLessThanOrEqual(1);
  expect(Math.abs(b.comp.x + b.comp.w / 2 - (b.thread.x + b.thread.w / 2))).toBeLessThanOrEqual(10);

  // The scroll's "↓" under the column's centre.
  await msgs(main).evaluate((m) => (m.scrollTop = 0));
  const down = main.getByTestId('chat-to-bottom');
  await expect(down).toBeVisible();
  const d = await rect(down);
  expect(Math.abs(d.x + d.w / 2 - (b.col.x + b.col.w / 2))).toBeLessThanOrEqual(2);
  await down.click();

  // A narrow thread (a 1,000 px window): its own width, as before.
  await main.setViewportSize({ width: 1000, height: 800 });
  await expect.poll(async () => (await columnBoxes(main)).col.w).toBeLessThan(COLUMN);
  b = await columnBoxes(main);
  expect(b.col.w).toBe(b.msgs.client - 4);
  expect(b.comp.w).toBe(b.thread.w - 40);
  await expectNothingOverlaps(main, 'a narrow thread');

  // A larger Text size: the column grows with it (a line keeps its length).
  await main.setViewportSize({ width: 1920, height: 1040 });
  await setConfig(yap, main, { readingSize: 'larger' });
  await expect.poll(async () => (await columnBoxes(main)).col.w).toBe(Math.round(COLUMN * 1.25));
  expect(Math.abs((await columnBoxes(main)).comp.w - Math.round(COLUMN * 1.25))).toBeLessThanOrEqual(1);
  await setConfig(yap, main, { readingSize: 'default' });
  await expect.poll(async () => (await columnBoxes(main)).col.w).toBe(COLUMN);
});

// ---------------------------------------------------------------- every size

test('every size, 900 to 2560 px: Home and Chat, a document and a Code chat’s agent beside it — nothing overlaps or cuts off', async ({ yap, main, fakeLlm, shot }) => {
  test.setTimeout(150_000);
  fs.rmSync(GARDEN, { recursive: true, force: true });
  fs.mkdirSync(GARDEN, { recursive: true });
  fs.writeFileSync(path.join(GARDEN, 'notes.md'), 'Draft notes\n');
  await yap.invoke('e2e_claude_approve_auto', { allow: true });
  await setConfig(yap, main, { agentsEnabled: true, agentsAsk: 'new-or-web', agentsSpeak: 'never', agentsPay: 'plan', agentsMode: 'auto' });
  if (!(await yap.invoke('workspaces_list')).some((f) => f.name === 'garden-planner')) await yap.invoke('workspaces_add', { path: GARDEN });
  const SIZES = [
    [900, 760],
    [1280, 800],
    [1920, 1040],
    [2560, 1400],
  ];

  // Home, the rail expanded and collapsed.
  await openView(main, 'Home');
  for (const [w, hgt] of SIZES) {
    await main.setViewportSize({ width: w, height: hgt });
    await expectNothingOverlaps(main, `Home at ${w}`);
  }
  await railBtn(main).click();
  for (const [w, hgt] of SIZES) {
    await main.setViewportSize({ width: w, height: hgt });
    await expectNothingOverlaps(main, `Home at ${w}, the rail collapsed`);
  }
  await railBtn(main).click();

  // Chat, a document beside it.
  await main.setViewportSize({ width: 1280, height: 800 });
  await openView(main, 'Chat');
  await toSide(main, 'Chat');
  await openDocument(main, fakeLlm, 'Pond edge');
  for (const [w, hgt] of SIZES) {
    await main.setViewportSize({ width: w, height: hgt });
    await expectNothingOverlaps(main, `Chat at ${w}, a document open`);
    await shot(main, `15-chat-document-${w}`);
  }
  await panel(main).getByRole('button', { name: 'Close the document' }).click();

  // The Code side: an agent starting, its tab beside the chat.
  await main.setViewportSize({ width: 1920, height: 1040 });
  await yap.invoke('e2e_claude_script', { script: writeScript('slow', SLOW) });
  fakeLlm.scriptChat([
    { tools: [{ name: 'start_agent', args: { folder: 'garden-planner', brief: 'Read notes.md slowly.' } }] },
    { content: "On it: I've sent an agent to garden-planner." },
  ]);
  await toSide(main, 'Code');
  await main.getByRole('button', { name: 'New code chat' }).click();
  await main.getByTestId('chat-workspace-menu').getByRole('menuitemradio', { name: 'garden-planner', exact: true }).click();
  await send(main, 'Send an agent to garden-planner.');
  await expect(agentTab(main)).toBeVisible();
  await expect(main.locator('.cbubble.assistant').last()).toContainText("On it: I've sent an agent to garden-planner.");
  // At 1,920 nothing needs to give way: the rail and the list stay, the column centred beside the tab.
  expect(await railWidth(main)).toBe(RAIL_WIDE);
  await expect(list(main)).toBeVisible();
  const b = await columnBoxes(main);
  expect(b.col.w).toBe(COLUMN);
  expect(Math.abs(b.col.x - b.comp.x)).toBeLessThanOrEqual(1);
  await shot(main, '16-code-agent-tab-1920-light');
  await theme(yap, main, 'dark');
  await shot(main, '17-code-agent-tab-1920-dark');
  await theme(yap, main, 'light');
  // Everything collapsed beside it.
  await railBtn(main).click();
  await box(main).click();
  await main.keyboard.press('Control+b');
  await expect.poll(() => railWidth(main)).toBe(RAIL_NARROW);
  await shot(main, '18-code-agent-tab-1920-everything-collapsed');
  await railBtn(main).click();
  await main.keyboard.press('Control+b');
  for (const [w, hgt] of SIZES) {
    await main.setViewportSize({ width: w, height: hgt });
    await expectNothingOverlaps(main, `the Code side at ${w}, an agent's tab open`);
    await shot(main, `19-code-agent-tab-${w}`);
  }
  await main.setViewportSize({ width: 1280, height: 800 });
  await agentTab(main).getByRole('button', { name: 'Stop' }).click();
  await expect(main.getByTestId('chat-agent-status')).toHaveText('Stopped', { timeout: 20_000 });
  await agentTab(main).getByRole('button', { name: 'Close the panel' }).click();
  await toSide(main, 'Chat');
  await setConfig(yap, main, { agentsEnabled: false });
});
