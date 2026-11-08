// Yap Lab's control process (docs/lab.md): started by `lab start`, it runs
// the test Yap (the e2e harness's launchYap: portable data folder, its own
// WebView2 profile and CDP port, test mode), holds the CDP connection, and
// answers the CLI's commands on 127.0.0.1 with the token in control.json.
// It quits with `lab stop`, after `--idle` minutes without a command, or
// when the test Yap exits; the test Yap quits when it does (its stdin
// closes).
//
// Never: %APPDATA%\yap (the test Yap is portable, in the lab's folder), the
// person's config, keys, memory or chats, a sign-in, a click or key on the
// screen (Playwright's go to the test Yap's webviews over CDP).
import crypto from 'node:crypto';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { APP, labConfig, onOff, parseModel } from './options.mjs';
import { drawPng, pngSize } from './png.mjs';
import { templated } from './fake.mjs';
import { captureElement, captureLivePage, capturePanel, captureThread, captureWindow, livePage, pinTheme, shotNames, slug, themeNow, themesOf } from './shots.mjs';
import { makeSheet } from './sheet.mjs';
import { conversationOf, endOf, cardsOf, pendingOf, summarize, toolsOf, cut } from './turns.mjs';
import { makeWorkspace, NAME as WORKSPACE_NAME } from './workspace.mjs';

const LAB = path.resolve(process.argv[2] ?? '');
const START = JSON.parse(fs.readFileSync(path.join(LAB, 'start.json'), 'utf8'));
const CONTROL = path.join(LAB, 'control.json');
const RECORD = path.join(LAB, 'record.jsonl');
const RUNS = path.join(LAB, 'runs');
const TURNS = path.join(LAB, 'turns');
const SHOTS = path.join(LAB, 'shots');
const REPORTS = path.join(LAB, 'reports');
const PROMPTS = path.join(path.dirname(fileURLToPath(import.meta.url)), 'prompts');
/** The windows a shot can be of. */
const WINDOWS = ['thread', 'main', 'panel', 'page', 'bar'];

const harness = (file) => import(pathToFileURL(path.join(APP, 'e2e', 'support', file)).href);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log(new Date().toISOString(), ...a);

let control = { state: 'starting', pid: process.pid, startedAt: Date.now() };
function writeControl(patch) {
  control = { ...control, ...patch };
  fs.writeFileSync(CONTROL, JSON.stringify(control, null, 2));
}

/** @type {import('../../desktop/e2e/support/yap.js').YapApp} */
let yap = null;
let fakeLlm = null;
let fakeWeb = null;
let stopping = false;
let lastCommand = Date.now();
let workspace = null;

// ---------------------------------------------------------------- the record

/** Every item of the test Yap's record so far, and where the file was read to. */
const items = [];
let readTo = 0;
let partial = '';

function readRecord() {
  let fd;
  try {
    fd = fs.openSync(RECORD, 'r');
  } catch {
    return;
  }
  try {
    const size = fs.fstatSync(fd).size;
    if (size <= readTo) return;
    const buf = Buffer.alloc(size - readTo);
    fs.readSync(fd, buf, 0, buf.length, readTo);
    readTo = size;
    const text = partial + buf.toString('utf8');
    const lines = text.split('\n');
    partial = lines.pop();
    for (const line of lines) {
      if (!line.trim()) continue;
      try {
        const it = JSON.parse(line);
        items.push(it);
        if (turn && !turn.finalized && it.seq > turn.since) turn.items.push(it);
      } catch {
        /* a line cut short: never written whole */
      }
    }
  } finally {
    fs.closeSync(fd);
  }
}
const lastSeq = () => (items.length ? items[items.length - 1].seq : 0);

// ---------------------------------------------------------------- turns

/** The turn going now, or the last one. */
let turn = null;
let turns = 0;

class Turn {
  constructor({ question, spoken, since, askedAt, autoCards = null, report = null, shots = true, themes = ['light', 'dark'] }) {
    this.n = ++turns;
    this.question = question;
    this.spoken = spoken;
    this.since = since;
    this.askedAt = askedAt;
    this.autoCards = autoCards;
    this.report = report;
    // Its screenshots (shotSet): after it ends, and while a card waits.
    this.shotsOn = shots;
    this.themes = themes;
    this.shots = [];
    this.shotErrors = [];
    this.shotCards = new Set();
    this.items = [];
    this.status = 'running';
    this.error = null;
    this.conversation = null;
    this.startedAt = Date.now();
    this.endedAt = null;
    this.finalized = false;
    this.answered = new Set();
    this.autoAnswered = [];
    this.result = null;
    this.approvals = [];
  }
}

async function invoke(cmd, args) {
  return yap.invoke(cmd, args);
}
const tryInvoke = (cmd, args, fallback = null) => invoke(cmd, args).catch(() => fallback);

async function agentsNow() {
  return START.yapCode ? (await tryInvoke('agent_list', {}, [])) ?? [] : [];
}

/** One look at the turn: has it ended, or is a card waiting? */
async function tick() {
  readRecord();
  const t = turn;
  if (!t || t.finalized || t.ticking) return;
  t.ticking = true;
  try {
    t.conversation ??= conversationOf(t.items);
    const end = endOf(t.items, { spoken: t.spoken });
    if (end.done) {
      t.status = end.status;
      t.error = end.error;
      t.endedAt = Date.now();
      await finalize(t);
      return;
    }
    t.approvals = (await tryInvoke('claude_approve_pending', {}, [])) ?? [];
    const agents = await agentsNow();
    const pending = pendingOf(cardsOf(t.items, { approvals: t.approvals, agents })).filter((c) => !t.answered.has(c.id));
    if (!pending.length) {
      t.status = 'running';
      return;
    }
    // A card waiting: shots of it (once a card), before it's answered.
    const fresh = pending.filter((c) => !t.shotCards.has(c.id));
    if (t.shotsOn && fresh.length) {
      for (const c of fresh) t.shotCards.add(c.id);
      await shotSet(t, { set: `card:${fresh.map((c) => c.id).join('+')}`, prefix: `card-${slug(fresh[0].id)}-` }).catch((e) => t.shotErrors.push(`card shots: ${e.message.split('\n')[0]}`));
    }
    if (t.autoCards) {
      for (const card of pending) {
        const how = t.autoCards;
        await answerCard(card, how).catch((e) => log('auto answer failed', e.message));
        t.autoAnswered.push({ id: card.id, label: card.label, site: card.site ?? '', answer: how });
      }
      return;
    }
    t.status = 'waiting';
  } finally {
    t.ticking = false;
  }
}

/** The turn as it is now (`finalize`'s result once it's over). */
async function snapshot(t) {
  if (t.finalized) return t.result;
  const conv = t.conversation != null ? await tryInvoke('chat_get', { id: t.conversation }) : null;
  const pages = (await tryInvoke('e2e_page_views', {}, [])) ?? [];
  const out = summarize({ turn: t, items: t.items, conv, pages, approvals: t.approvals, agents: await agentsNow() });
  return withShots(t, out);
}

/** A turn's result with its shots (and what couldn't be shot). */
function withShots(t, out) {
  out.shots = t.shots.map(shotEntry);
  if (t.shotErrors.length) out.shotErrors = [...t.shotErrors];
  return out;
}

async function finalize(t) {
  // Its last events (a card's end, the page's chips) a moment after its end.
  await sleep(300);
  readRecord();
  const conv = t.conversation != null ? await tryInvoke('chat_get', { id: t.conversation }) : null;
  const pages = (await tryInvoke('e2e_page_views', {}, [])) ?? [];
  const agents = await agentsNow();
  fs.mkdirSync(TURNS, { recursive: true });
  const transcript = path.join(TURNS, `${String(t.n).padStart(3, '0')}.json`);
  const result = summarize({ turn: t, items: t.items, conv, pages, approvals: [], agents, transcript });
  if (t.autoAnswered.length) result.autoAnswered = t.autoAnswered;
  t.status = result.status;
  // Its shots: the thread's turn whole, the panel, the live page, in each theme.
  if (t.shotsOn) await shotSet(t, { set: 'end' }).catch((e) => t.shotErrors.push(`shots: ${e.message.split('\n')[0]}`));
  withShots(t, result);
  if (t.report) {
    t.reportFile ??= reportFile(t.report.name);
    let sheet = null;
    if (t.shots.length) {
      sheet = await makeSheet({ lab: LAB, turns: (n) => n === t.n, out: t.reportFile.replace(/\.md$/, '.sheet.png'), title: `Yap Lab report: ${t.report.name}`, subtitle: sheetSubtitle() }).catch((e) => ({ error: e.message.split('\n')[0] }));
      result.sheet = sheet;
    }
    result.report = writeReport(t, result, conv, sheet);
  }
  t.transcriptFile = transcript;
  t.transcriptData = { result, tools: toolsOf(t.items, { full: true }), conversation: conv, pages, agents, record: t.items };
  fs.writeFileSync(transcript, JSON.stringify(t.transcriptData, null, 2));
  t.result = result;
  t.finalized = true;
  writeShotIndex(t);
  log(`turn ${t.n} ${t.status} in ${result.ms} ms`);
}

// ---------------------------------------------------------------- shots

/** A shot as the turn's JSON lists it. */
function shotEntry(s) {
  return { file: s.file, window: s.window, theme: s.theme, set: s.set, width: s.width, height: s.height, dpr: s.dpr ?? 1, ...(s.pieces ? { pieces: s.pieces, cssHeight: s.cssHeight, scrollHeight: s.scrollHeight, clientHeight: s.clientHeight } : {}) };
}

const turnShotDir = (t) => path.join(SHOTS, String(t.n).padStart(3, '0'));

/** `shots/NNN/index.json`: the turn and its shots, for `sheet`; and the transcript's list once it's written. */
function writeShotIndex(t) {
  if (!t.shots.length) return;
  fs.mkdirSync(turnShotDir(t), { recursive: true });
  fs.writeFileSync(
    path.join(turnShotDir(t), 'index.json'),
    JSON.stringify({ turn: t.n, question: t.question, status: t.result?.status ?? t.status, conversation: t.conversation, spoken: !!t.spoken, shots: t.shots }, null, 2)
  );
  if (t.finalized && t.transcriptData) {
    withShots(t, t.result);
    fs.writeFileSync(t.transcriptFile, JSON.stringify(t.transcriptData, null, 2));
  }
}

const sheetSubtitle = () => [START.model ? `${START.model.provider} ${START.model.model}` : '', START.fake ? 'fake' : '', START.build?.commit ? `build ${String(START.build.commit).slice(0, 9)}` : '', new Date().toISOString().slice(0, 16).replace('T', ' ')].filter(Boolean).join(' · ');

/** One shot set at a time (each one marks the theme on the windows). */
let shooting = Promise.resolve();
function oneAtATime(fn) {
  const run = shooting.then(fn, fn);
  shooting = run.catch(() => {});
  return run;
}

/** Chat's thread showing `t`'s conversation (the question in it), as Yap's own "Open in Chat" opens one. */
async function showTurn(t) {
  if (t.conversation == null) return;
  const has = () =>
    main().evaluate((q) => {
      const norm = (s) => String(s ?? '').replace(/\s+/g, ' ').trim().toLowerCase();
      const want = norm(q).slice(0, 40);
      const col = document.querySelector('[data-testid="chat-column"]');
      return !!col && [...col.querySelectorAll('.cbubble.user')].some((b) => norm(b.innerText).includes(want));
    }, t.question);
  if (await has().catch(() => false)) return;
  await onScreen();
  await openChatView().catch(() => {});
  await main().evaluate((p) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', { event: 'yap-assistant-turn', payload: p }), { phase: 'open', conversationId: t.conversation, show: true }).catch(() => {});
  await waitFor(has, 5_000, `conversation ${t.conversation} to show`).catch(() => {});
}

/** One window's capture in `theme` (the theme's already marked): `{ png, dpr, … }`, or an error saying what wasn't there. */
async function take(window, { t, theme, page, selector }) {
  if (window === 'thread') {
    const got = await captureThread(main(), { question: t?.question ?? '' });
    if (!got) throw new Error('Chat’s thread isn’t showing');
    return got;
  }
  if (window === 'main') return selector ? need(await captureElement(main(), selector), `nothing in the main window matches ${selector}`) : captureWindow(main());
  if (window === 'panel') {
    const got = selector ? await captureElement(main(), `[data-testid="chat-panel-slot"] ${selector}`) : await capturePanel(main());
    return need(got, selector ? `nothing in the panel matches ${selector}` : 'Chat has no panel showing now');
  }
  if (window === 'bar') return selector ? need(await captureElement(yap.overlay, selector), `nothing on the bar matches ${selector}`) : captureWindow(yap.overlay);
  if (window === 'page') {
    if (!page) throw new Error("No page beside the chat to take a shot of (or it isn't reachable over DevTools: use --window panel)");
    return captureLivePage(page, theme);
  }
  throw new Error(`--window is ${WINDOWS.join(', ')}`);
}
const need = (v, why) => {
  if (!v) throw new Error(why);
  return v;
};

/**
 * **A set of shots**: `windows` (by default what's there: the thread, the
 * panel when it's open, the live page when one's on screen), each in each
 * of `themes` (null: as it is), into `t`'s folder (`shots/NNN/`) or, with no
 * turn, `shots/`. Named `<prefix>01-thread-light.png`…; listed in the turn's
 * JSON and its `index.json`. `{ shots, errors }`.
 */
async function shotSet(t, { set = 'end', prefix = '', windows = null, themes = t?.themes ?? themesOf(START.theme), selector = null, explicit = false } = {}) {
  return oneAtATime(async () => {
    const dir = t ? turnShotDir(t) : SHOTS;
    fs.mkdirSync(dir, { recursive: true });
    const views = (await tryInvoke('e2e_page_views', {}, [])) ?? [];
    const page = livePage(yap, views, { onScreenOnly: !explicit });
    let list = windows;
    if (!list) {
      if (t) await showTurn(t);
      list = [];
      if (await main().getByTestId('chat-column').isVisible().catch(() => false)) list.push('thread');
      if (await main().getByTestId('chat-panel-slot').isVisible().catch(() => false)) list.push('panel');
      if (page) list.push('page');
    }
    const names = shotNames(list, themes.map((x) => x ?? 'as-is'), { prefix });
    const shots = [];
    const errors = [];
    try {
      for (const theme of themes) {
        if (theme) {
          await pinTheme(main(), theme);
          if (list.includes('bar')) await pinTheme(yap.overlay, theme);
        }
        const shown = theme ?? (await themeNow(main()).catch(() => 'light'));
        for (const n of names.filter((x) => x.theme === (theme ?? 'as-is'))) {
          try {
            const got = await take(n.window, { t, theme, page, selector });
            const file = path.join(dir, theme ? n.file : n.file.replace(/-as-is\.png$/, '.png'));
            fs.writeFileSync(file, got.png);
            const { width, height } = pngSize(got.png);
            const { png, ...about } = got;
            shots.push({ ...about, file, set, window: n.window, theme: shown, width, height, order: names.indexOf(n) });
          } catch (e) {
            errors.push(`${n.window}${theme ? ` (${theme})` : ''}: ${e.message.split('\n')[0]}`);
          }
        }
      }
    } finally {
      await pinTheme(main(), null).catch(() => {});
      if (list.includes('bar')) await pinTheme(yap.overlay, null).catch(() => {});
    }
    shots.sort((a, b) => a.order - b.order);
    for (const s of shots) delete s.order;
    if (t) {
      t.shots.push(...shots);
      t.shotErrors.push(...errors);
      writeShotIndex(t);
    }
    return { shots, errors };
  });
}

/** Wait until the turn ends or a card waits, for at most `secs`. */
async function settle(t, secs) {
  const deadline = Date.now() + secs * 1000;
  while (Date.now() < deadline) {
    await tick();
    if (t.finalized) return t.result;
    if (t.status === 'waiting') return snapshot(t);
    await sleep(300);
  }
  return snapshot(t);
}

// ---------------------------------------------------------------- the UI

const main = () => yap.main;

async function onScreen() {
  await tryInvoke('e2e_main_on_screen', {});
  const settings = main().getByRole('dialog', { name: 'Settings' });
  if (await settings.isVisible().catch(() => false)) {
    await settings.getByRole('button', { name: 'Close settings' }).click({ timeout: 5_000 }).catch(() => {});
  }
}

async function openChatView() {
  const item = main().getByRole('navigation', { name: 'Main' }).getByRole('button', { name: 'Chat', exact: true });
  await item.click({ timeout: 10_000 });
  await waitFor(async () => (await item.getAttribute('aria-current')) === 'page', 10_000, 'the Chat view to open');
}

async function waitFor(check, ms, what) {
  const deadline = Date.now() + ms;
  while (Date.now() < deadline) {
    if (await check().catch(() => false)) return;
    await sleep(150);
  }
  throw new Error(`Timed out waiting for ${what}`);
}

/** Chat's list on the Chat or Code side (only with Yap Code on). */
async function toSide(side) {
  const sw = main().getByTestId('chat-side-switch');
  if (!(await sw.isVisible().catch(() => false))) {
    if (side === 'Code') throw new Error('No Code side: start the lab with --yap-code on');
    return;
  }
  const radio = sw.getByRole('radio', { name: side, exact: true });
  if ((await radio.getAttribute('aria-checked')) !== 'true') await radio.click();
  await waitFor(async () => (await radio.getAttribute('aria-checked')) === 'true', 5_000, `the ${side} side`);
}

/** Chat's list of chats on screen (a panel beside the chat can push it aside). */
async function showList() {
  const show = main().getByTestId('chat-list-show');
  if (await show.isVisible().catch(() => false)) {
    await show.click({ timeout: 5_000 });
    await waitFor(async () => !(await show.isVisible()), 5_000, 'the list of chats');
  }
}

/** Chat open on what `chat` names: "new", a conversation's id, or "code:<workspace>". */
async function openChat(chat) {
  await onScreen();
  await openChatView();
  await showList();
  const m = main();
  if (!chat || chat === 'new') {
    await toSide('Chat');
    await m.getByRole('button', { name: 'New chat' }).first().click({ timeout: 10_000 });
    return { askedAt: 0 };
  }
  if (String(chat).startsWith('code:')) {
    const name = String(chat).slice(5) || workspace?.name;
    await toSide('Code');
    await m.getByRole('button', { name: 'New code chat' }).first().click({ timeout: 10_000 });
    const menu = m.getByTestId('chat-workspace-menu');
    await menu.getByRole('menuitemradio', { name, exact: true }).click({ timeout: 10_000 });
    await waitFor(async () => (await m.getByTestId('chat-workspace-chip').getAttribute('data-workspace')) === name, 10_000, `the workspace ${name}`);
    return { askedAt: 0 };
  }
  const id = Number(chat);
  if (!Number.isFinite(id)) throw new Error(`--chat is new, a conversation's id or code:<workspace>, not "${chat}"`);
  const conv = await invoke('chat_get', { id });
  // As Yap's own "Open in Chat" does: the Chat view follows this event.
  await m.evaluate((p) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', { event: 'yap-assistant-turn', payload: p }), { phase: 'open', conversationId: id, show: true });
  await waitFor(async () => (await m.locator(`.conv.active[data-conv="${id}"]`).count()) > 0, 10_000, `conversation ${id} to open`);
  return { askedAt: conv?.messages?.length ?? 0 };
}

/**
 * `--chat same`: the last turn's conversation — left as it is when Chat
 * still shows it (its question in the thread), else opened by its id.
 */
async function sameChat() {
  if (!turn?.conversation) throw new Error('--chat same needs an earlier turn in a conversation');
  const id = turn.conversation;
  const conv = await invoke('chat_get', { id });
  const shown = await main()
    .evaluate((q) => {
      const norm = (s) => String(s ?? '').replace(/\s+/g, ' ').trim().toLowerCase();
      const want = norm(q).slice(0, 40);
      const col = document.querySelector('[data-testid="chat-column"]');
      return !!col && [...col.querySelectorAll('.cbubble.user')].some((b) => norm(b.innerText).includes(want));
    }, turn.question)
    .catch(() => false);
  if (shown) return { askedAt: conv?.messages?.length ?? 0 };
  return openChat(String(id));
}

/** `--shots` / `--no-shots` and `--theme` for a turn: the lab's defaults (`start`) unless the command says. */
function shotsFor(a) {
  const on = a['no-shots'] ? false : a.shots !== undefined ? onOff(a.shots, true, 'shots') : START.shots !== false;
  return { shots: on, themes: themesOf(a.theme ?? START.theme) };
}

async function startTurn({ text, spoken = false, chat = 'new', autoCards = null, report = null, shots = true, themes }) {
  if (!text?.trim()) throw new Error('Nothing to ask');
  if (turn && !turn.finalized) throw new Error(`Turn ${turn.n} is still going (${turn.status}): wait for it, answer its card, or cancel it`);
  readRecord();
  const since = lastSeq();
  const { askedAt } = await (chat === 'same' ? sameChat() : openChat(chat)).catch((e) => {
    throw new Error(`Couldn't open Chat on ${chat || 'a new chat'}: ${e.message.split('\n')[0]}`);
  });
  const t = new Turn({ question: text, spoken, since, askedAt, autoCards, report, shots, themes });
  turn = t;
  try {
    if (spoken) {
      await invoke('e2e_assistant_ask', { text, listenMs: 300 });
    } else {
      const box = main().getByRole('textbox', { name: 'Message' });
      await box.fill(text, { timeout: 10_000 });
      // (Exactly "Send": with an agent's tab beside the chat, other buttons' names have the word.)
      await main().getByRole('button', { name: 'Send', exact: true }).click({ timeout: 10_000 });
    }
  } catch (e) {
    t.status = 'failed';
    t.error = `Couldn't ask: ${e.message.split('\n')[0]}`;
    t.endedAt = Date.now();
    await finalize(t);
  }
  return t;
}

// ---------------------------------------------------------------- cards

/** Answer `card` as the person would: "allow" (Allow once / Go ahead), "deny", or "always" (a connector's Always allow). */
async function answerCard(card, how) {
  const [kind, raw] = card.id.split(/:(.*)/s);
  const allow = how !== 'deny';
  let answered;
  if (kind === 'confirm') {
    answered = card.kind === 'connector'
      ? await invoke('chat_connector_confirm', { id: Number(raw), answer: how === 'always' ? 'always' : allow ? 'once' : 'deny' })
      : await invoke('chat_page_confirm', { id: Number(raw), allow });
  } else if (kind === 'approve') {
    answered = await invoke('claude_approve_answer', { id: Number(raw), allow });
  } else if (kind === 'plan') {
    answered = !!(allow ? await invoke('agent_go_ahead', { id: raw }) : await invoke('agent_stop', { id: raw }));
  } else {
    throw new Error(`No card ${card.id}`);
  }
  turn?.answered.add(card.id);
  return answered;
}

// ---------------------------------------------------------------- reports

function reportFile(name) {
  const stamp = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19);
  return path.join(REPORTS, `${name}-${stamp}.md`);
}

/** The report's "Screenshots" section: the sheet, and a link to each shot (paths from the report's folder). */
function shotsSection(t, sheet) {
  if (!t.shots.length) return [];
  const rel = (f) => path.relative(REPORTS, f).split(path.sep).join('/');
  return [
    '## Screenshots',
    '',
    sheet?.png ? `![The turn's contact sheet](${rel(sheet.png)})\n\n[The sheet as a page](${rel(sheet.html)})` : sheet?.error ? `No contact sheet: ${sheet.error}` : '',
    '',
    ...t.shots.map((s) => `- [${path.basename(s.file)}](${rel(s.file)}): ${s.window}, ${s.theme}${s.set && s.set !== 'end' ? `, ${s.set}` : ''}, ${s.width}×${s.height}`),
    ...(t.shotErrors.length ? ['', ...t.shotErrors.map((e) => `- Not shot: ${e}`)] : []),
    '',
  ];
}

function writeReport(t, result, conv, sheet = null) {
  fs.mkdirSync(REPORTS, { recursive: true });
  const file = t.reportFile ?? reportFile(t.report.name);
  const counts = new Map();
  for (const tool of result.tools) {
    const c = counts.get(tool.name) ?? { calls: 0, failed: 0, refused: 0 };
    c.calls++;
    if (tool.ok === false) c.failed++;
    if (tool.refused) c.refused++;
    counts.set(tool.name, c);
  }
  const rows = [...counts.entries()].map(([name, c]) => `| ${name} | ${c.calls} | ${c.failed} | ${c.refused} |`);
  const calls = toolsOf(t.items).map((tool, i) => `${i + 1}. \`${tool.name}\`${tool.via === 'claude' && tool.server ? ` (${tool.server})` : ''} ${tool.ok === false ? (tool.refused ? '**refused**' : '**failed**') : 'ok'}: ${cut(String(tool.args ?? ''), 160).replace(/\n/g, ' ')} → ${cut(String(tool.result ?? ''), 240).replace(/\n/g, ' ')}`);
  const model = result.model ? `${result.model.provider ?? ''} ${result.model.id ?? ''}`.trim() : START.model?.model ?? '';
  const text = [
    `# Yap Lab report: ${t.report.name}`,
    '',
    `- When: ${new Date(t.startedAt).toISOString()}, ${Math.round(result.ms / 1000)} s`,
    `- Model: ${model}${START.effort ? ` at ${START.effort}` : ''}${START.fake ? ' (fake)' : ''}`,
    `- Build: ${START.build?.commit ?? 'unknown'}`,
    `- Turn: ${result.status}${result.error ? ` — ${result.error}` : ''} (conversation ${result.conversation ?? '?'}; transcript ${result.transcript})`,
    t.autoAnswered.length ? `- Cards answered by the lab (${t.autoCards}): ${t.autoAnswered.map((c) => `“${c.label}” on ${c.site || '?'}`).join('; ')}` : '- No cards were asked.',
    '',
    '## The model’s report',
    '',
    conv ? (conv.messages ?? []).at(-1)?.text ?? result.answer : result.answer,
    '',
    '## Tool calls the lab recorded',
    '',
    rows.length ? ['| Tool | Calls | Failed | Refused |', '|---|---|---|---|', ...rows].join('\n') : 'None.',
    '',
    ...calls,
    '',
    ...shotsSection(t, sheet),
  ].join('\n');
  fs.writeFileSync(file, text);
  return file;
}

function promptFile(name) {
  const base = String(name ?? '').replace(/\.md$/, '');
  if (!/^[a-z0-9-]+$/i.test(base)) throw new Error(`No prompt "${name}"`);
  const file = path.join(PROMPTS, `${base}.md`);
  if (!fs.existsSync(file)) {
    const have = fs.readdirSync(PROMPTS).filter((f) => f.endsWith('.md')).map((f) => f.replace(/\.md$/, ''));
    throw new Error(`No prompt "${base}": there are ${have.join(', ')}`);
  }
  return { name: base, file };
}

// ---------------------------------------------------------------- commands

/**
 * `shot [--window …] [--theme light|dark|both] [--name <n>] [--selector <css>]`:
 * a window as it is (`shots/<time>-<window>.png`), or in a theme, or both;
 * `--name`: into the current turn's folder, listed with its shots (the
 * sheet shows it). `thread`: the turn whole, scrolled and stitched.
 */
async function shot({ window: which = 'main', theme, name, selector }) {
  if (!WINDOWS.includes(which)) throw new Error(`--window is ${WINDOWS.join(', ')}`);
  const themes = theme === undefined ? [null] : themesOf(theme);
  const stamp = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19);
  const t = name ? turn : null;
  const prefix = name ? `${slug(name)}-` : `${stamp}-`;
  const set = name ? `named:${name}` : 'manual';
  const out = await shotSet(t, { set, prefix, windows: [which], themes, selector: selector ?? null, explicit: true });
  if (!out.shots.length) throw new Error(out.errors.join('; ') || 'Nothing to take a shot of');
  // An unnamed shot as it is keeps its old name: <time>-<window>.png.
  if (!name && themes[0] === null && out.shots.length === 1) {
    const file = path.join(SHOTS, `${stamp}-${which}.png`);
    fs.renameSync(out.shots[0].file, file);
    out.shots[0].file = file;
  }
  return { shot: out.shots[0].file, window: which, shots: out.shots.map(shotEntry), ...(out.errors.length ? { errors: out.errors } : {}) };
}

/**
 * `probe`: is `css` in `window` (count, and how many show), and is `text`
 * in it (the words as shown: hidden text doesn't count)? `thread`: Chat's
 * thread; `panel`: the panel beside the chat; `page`: the live page.
 */
async function probe({ window: which = 'main', css, text }) {
  if (!css && !text) throw new Error('probe needs --css or --text');
  const views = (await tryInvoke('e2e_page_views', {}, [])) ?? [];
  const page = which === 'bar' ? yap.overlay : which === 'page' ? livePage(yap, views) : main();
  if (!page) throw new Error('No page beside the chat');
  const scope = which === 'panel' ? '[data-testid="chat-panel-slot"]' : which === 'thread' ? '[data-testid="chat-column"]' : null;
  const got = await page.evaluate(
    ([scope, css, text]) => {
      const root = scope ? document.querySelector(scope) : document.body;
      if (!root) return { found: false, count: 0, visible: 0, ...(text ? { text: false } : {}) };
      const out = { found: true, theme: document.documentElement.dataset.yapTheme ?? null };
      if (css) {
        const els = [...root.querySelectorAll(css)];
        out.count = els.length;
        out.visible = els.filter((e) => e.getClientRects().length > 0 && getComputedStyle(e).visibility !== 'hidden').length;
      }
      if (text) {
        const norm = (s) => String(s ?? '').replace(/\s+/g, ' ').trim().toLowerCase();
        out.text = norm(root.innerText).includes(norm(text));
      }
      return out;
    },
    [scope, css ?? null, text ?? null]
  );
  const { text: has, ...rest } = got;
  return { window: which, ...(css ? { css } : {}), ...rest, ...(text ? { words: cut(text, 120), has } : {}) };
}

/** Config keys `set` never touches (keys, sign-ins, Chat's model: `start` sets those). */
const NEVER_SET = /key|token|secret|password|llmScopes|auth/i;

/** The test Yap's config patched as another surface would (fixtures.js `setConfig`): saved, Settings told, checked. */
async function patchConfig(patch) {
  const cfg = await invoke('get_config');
  await invoke('save_config', { cfg: { ...cfg, ...patch } });
  await main().evaluate((p) => window.dispatchEvent(new CustomEvent('yap-config-patched', { detail: p })), patch);
  await sleep(700);
  const now = await invoke('get_config');
  const off = Object.keys(patch).filter((k) => JSON.stringify(now[k]) !== JSON.stringify(patch[k]));
  if (off.length) throw new Error(`The config didn't keep ${off.join(', ')}`);
}

/** A workspace file `rel` (inside the workspace only) written: `text`, or a made-up picture (`png`: drawPng's options). */
function seedFile(dir, { path: rel, text, png }) {
  if (!rel || path.isAbsolute(rel)) throw new Error('A file to seed has a path inside the workspace (relative)');
  const root = path.resolve(dir);
  const file = path.resolve(root, rel);
  if (!file.startsWith(root + path.sep)) throw new Error(`${rel} isn't inside the workspace`);
  fs.mkdirSync(path.dirname(file), { recursive: true });
  if (png) fs.writeFileSync(file, drawPng(png));
  else fs.writeFileSync(file, String(text ?? ''));
  return path.relative(root, file).split(path.sep).join('/');
}

/**
 * `set`: the web, Yap Code, connectors or other settings on or off in the
 * running test Yap (`config`: other keys, never a key, a sign-in or Chat's
 * model), and files seeded into the scratch workspace (`files`: `{ path,
 * text }` or `{ path, png: { width, height, shape, color, background } }`).
 * `bypass`: new chats' Mode Bypass permissions (on: the test Yap's own
 * one-time OK given, as the person gives it in the Mode menu) or Auto (off).
 */
async function setThings(a) {
  const patch = {};
  const done = {};
  if (a.web !== undefined) patch.webSearch = onOff(a.web, true, 'web');
  if (a.connectors !== undefined) patch.connectorsPreview = onOff(a.connectors, false, 'connectors');
  if (a.bypass !== undefined) {
    const on = onOff(a.bypass, false, 'bypass');
    if (on) await invoke('agent_bypass_confirmed');
    patch.agentsMode = on ? 'bypassPermissions' : 'auto';
  }
  if (a.config && typeof a.config === 'object') {
    for (const [k, v] of Object.entries(a.config)) {
      if (NEVER_SET.test(k)) throw new Error(`set doesn't change ${k} (start sets the model and keys)`);
      patch[k] = v;
    }
  }
  const yapCode = a['yap-code'] ?? a.yapCode;
  if (yapCode !== undefined) {
    const on = onOff(yapCode, false, 'yap-code');
    patch.agentsEnabled = on;
    if (on) Object.assign(patch, { agentsPay: START.model.kind === 'api' ? 'key' : 'plan', agentsSpeak: 'never' });
  }
  if (Object.keys(patch).length) {
    await patchConfig(patch);
    done.config = patch;
  }
  if (yapCode !== undefined) {
    START.yapCode = onOff(yapCode, false, 'yap-code');
    if (START.yapCode) {
      const status = await invoke('claude_plan_status', { refresh: true });
      const code = status?.code ?? status ?? {};
      if (!code.installed || !code.loggedIn) {
        throw new Error(`Yap Code is on, but Claude Code isn't ${code.installed ? 'signed in' : 'here'}${START.fake ? ' (a --fake lab has its stand-in only when started with --yap-code on)' : ''}`);
      }
      if (!workspace) {
        const dir = START.workspace ? path.resolve(START.workspace) : makeWorkspace(path.join(LAB, 'workspace', WORKSPACE_NAME), { keep: !!START.keepWorkspace });
        const added = await invoke('workspaces_add', { path: dir });
        workspace = { name: added?.name ?? path.basename(dir), path: dir };
      }
      done.workspace = workspace;
    }
  }
  const files = a.files ?? (a.file ? [a.file] : []);
  if (files.length) {
    if (!workspace) throw new Error('No workspace to seed files in: start the lab with --yap-code on');
    // Only the lab's own scratch workspace: never a folder of the person's (`--workspace`).
    if (!path.resolve(workspace.path).startsWith(path.resolve(LAB) + path.sep)) throw new Error(`set only seeds the lab's scratch workspace, not ${workspace.path}`);
    done.files = files.map((f) => seedFile(workspace.path, f));
  }
  if (!Object.keys(done).length) throw new Error('Nothing to set: --web, --yap-code, --connectors, --bypass, config or files');
  return done;
}

async function state() {
  readRecord();
  const conversations = ((await tryInvoke('chats_list', {}, [])) ?? []).map((c) => ({ id: c.id, title: c.title, updated: c.updatedTs, messages: c.count ?? c.messages ?? undefined }));
  const pages = (await tryInvoke('e2e_page_views', {}, [])) ?? [];
  const agents = (await agentsNow()).map((a) => ({ id: a.id, folder: a.folder, status: a.status, conversation: a.conversation, plan: a.plan, text: cut(a.text ?? '', 200), files: a.files, error: a.error ?? null }));
  const approvals = (await tryInvoke('claude_approve_pending', {}, [])) ?? [];
  const cards = turn && !turn.finalized ? pendingOf(cardsOf(turn.items, { approvals, agents: await agentsNow() })).filter((c) => !turn.answered.has(c.id)) : cardsOf([], { approvals, agents: await agentsNow() });
  const opened = (await tryInvoke('e2e_opened_apps', {}, [])) ?? [];
  // The panel beside the chat: its tabs (documents, pictures, pages, agents), as its strip has them.
  const panelTabs = await main()
    .getByTestId('chat-panel-tab')
    .evaluateAll((tabs) => tabs.map((t) => ({ key: t.dataset.key, title: (t.getAttribute('aria-label') || t.textContent || '').trim().replace(/\s+/g, ' '), selected: t.getAttribute('aria-selected') === 'true' })))
    .catch(() => []);
  return {
    lab: { model: START.model, effort: START.effort ?? null, web: START.web, yapCode: START.yapCode, connectors: START.connectors, fake: !!START.fake, shots: START.shots !== false, theme: START.theme ?? 'both', size: START.size ?? '1280x860', dataDir: yap.dataDir, pid: yap.child.pid, upMs: Date.now() - control.startedAt, workspace },
    turn: turn ? { n: turn.n, status: turn.status, conversation: turn.conversation, question: cut(turn.question, 120) } : null,
    conversations: Array.isArray(conversations) ? conversations : [],
    panelTabs,
    pages,
    agents,
    pending: cards,
    appsOpened: opened,
  };
}

/** Yap Code's agents now: each one's state, the cards they wait on, and the reports they put in their chats. */
async function agentsView() {
  readRecord();
  const list = await agentsNow();
  const approvals = (await tryInvoke('claude_approve_pending', {}, [])) ?? [];
  const reports = [];
  for (const conv of [...new Set(list.map((x) => x.conversation).filter((c) => c != null))]) {
    const c = await tryInvoke('chat_get', { id: conv });
    const messages = c?.messages ?? [];
    messages.forEach((m, i) => {
      if (m.kind !== 'agentReport') return;
      const reply = messages.slice(i + 1).find((n) => n.role === 'assistant' && n.kind !== 'agentReport');
      reports.push({ conversation: conv, report: cut(m.text, 1500), answer: reply ? cut(reply.text, 1500) : null });
    });
  }
  return {
    agents: list.map((x) => ({ id: x.id, folder: x.folder, status: x.status, conversation: x.conversation, mode: x.mode, plan: x.plan, readOnly: !!x.turnList?.at(-1)?.readOnly, text: cut(x.text ?? '', 600), files: x.files, commands: x.commands, denials: x.denials, error: x.error ?? null, cost: x.cost })),
    pending: cardsOf([], { approvals, agents: list }).filter((c) => c.state === 'asking'),
    reports,
  };
}

/** Wait until no agent is queued or running (one waiting on a card counts as stopped), at most `secs`. */
async function waitAgents(secs) {
  const deadline = Date.now() + secs * 1000;
  await sleep(500);
  while (Date.now() < deadline) {
    const busy = (await agentsNow()).some((x) => ['queued', 'running'].includes(String(x.status).toLowerCase()));
    if (!busy) break;
    await sleep(1000);
  }
  // The report a finished agent puts in its chat comes a moment after.
  await sleep(1500);
  const view = await agentsView();
  const still = view.agents.some((x) => ['queued', 'running'].includes(String(x.status).toLowerCase()));
  return { ...view, ...(still ? { hint: 'Agents are still working: npm run lab -- wait --agents' } : view.pending.length ? { hint: `An agent is waiting: npm run lab -- answer ${view.pending[0].id} allow|deny` } : {}) };
}

/**
 * `wait --agents --cards allow|deny`: as `waitAgents`, answering every
 * card the agents ask on the way (a permission prompt, a plan's Go ahead),
 * each one's shots first (into the last turn's folder) unless `shots` is
 * off, until none is queued, running or asking, or `secs` pass.
 */
async function waitAgentsAnswering(secs, how, { shots = true, themes } = {}) {
  const deadline = Date.now() + secs * 1000;
  const answered = [];
  for (;;) {
    const view = await waitAgents(Math.max(1, (deadline - Date.now()) / 1000));
    const pend = view.pending ?? [];
    if (!pend.length || Date.now() >= deadline) return { ...view, ...(answered.length ? { autoAnswered: answered } : {}) };
    if (shots && turn) {
      await shotSet(turn, { set: `card:${pend.map((c) => c.id).join('+')}`, prefix: `card-${slug(pend[0].id)}-`, themes: themes ?? turn.themes }).catch((e) => turn.shotErrors.push(`card shots: ${e.message.split('\n')[0]}`));
    }
    for (const card of pend) {
      await answerCard(card, how).catch((e) => log('auto answer failed', e.message));
      answered.push({ id: card.id, label: card.label, answer: how });
    }
  }
}

function logs({ since, grep, tail }) {
  const all = fs.existsSync(yap.logPath) ? fs.readFileSync(yap.logPath, 'utf8').split(/\r?\n/) : [];
  if (all.at(-1) === '') all.pop();
  let from = since !== undefined ? Math.max(0, Number(since)) : Math.max(0, all.length - Number(tail ?? 200));
  let lines = all.slice(from).map((text, i) => ({ n: from + i, text }));
  if (grep) {
    const re = new RegExp(grep, 'i');
    lines = lines.filter((l) => re.test(l.text));
  }
  if (tail !== undefined && since !== undefined) lines = lines.slice(-Number(tail));
  return { file: yap.logPath, next: all.length, lines: lines.map((l) => `${l.n}: ${cut(l.text, 1000)}`) };
}

async function shutdown(why, end = 'stopped') {
  if (stopping) return;
  stopping = true;
  log(`stopping: ${why}`);
  if (control.state !== 'failed') writeControl({ state: 'stopping' });
  await yap?.stop().catch((e) => log('stop failed', e.message));
  await fakeLlm?.close().catch(() => {});
  await fakeWeb?.close().catch(() => {});
  writeControl({ state: end, stoppedAt: Date.now(), why });
  setTimeout(() => process.exit(0), 200);
}

const COMMANDS = {
  ping: async () => ({ pong: true }),
  ask: async (a) => {
    const t = await startTurn({ text: a.text, spoken: !!a.spoken, chat: a.chat ?? 'new', autoCards: ['allow', 'deny'].includes(a.cards) ? a.cards : null, ...shotsFor(a) });
    return a['no-wait'] ? snapshot(t) : settle(t, Number(a.timeout ?? 600));
  },
  wait: async (a) => {
    if (a.agents) {
      if (['allow', 'deny'].includes(a.cards)) {
        const { shots, themes } = shotsFor(a);
        return waitAgentsAnswering(Number(a.timeout ?? 600), a.cards, { shots, themes: a.theme ? themes : undefined });
      }
      return waitAgents(Number(a.timeout ?? 600));
    }
    if (!turn) throw new Error('No turn yet: ask something first');
    return settle(turn, Number(a.timeout ?? 600));
  },
  agents: async () => agentsView(),
  probe: async (a) => probe(a),
  set: async (a) => setThings(a),
  answer: async (a) => {
    const [id, how = 'allow'] = a._;
    if (!id) throw new Error('Which card? answer <card-id> allow|deny|always');
    if (!['allow', 'deny', 'always'].includes(how)) throw new Error('A card is answered allow, deny or always');
    readRecord();
    const approvals = (await tryInvoke('claude_approve_pending', {}, [])) ?? [];
    const cards = cardsOf(turn?.items ?? [], { approvals, agents: await agentsNow() });
    // (A card the record hasn't shown: answered by its kind.)
    const prefix = /^(confirm|approve|plan):/.exec(id)?.[1];
    const card = cards.find((c) => c.id === id) ?? (prefix ? { id, kind: prefix === 'confirm' ? 'page' : prefix } : null);
    if (!card) throw new Error(`No card ${id}: the cards are ${cards.map((c) => c.id).join(', ') || 'none'}`);
    if (how === 'always' && card.kind !== 'connector') throw new Error('Only a connector’s card has Always allow');
    const answered = await answerCard(card, how);
    const answeredNote = { answered: { id, how, wasWaiting: !!answered } };
    // An agent's card: the agents, as they go on.
    if (card.kind === 'approve' || card.kind === 'plan' || !turn || turn.finalized) {
      return { ...answeredNote, ...(a['no-wait'] ? await agentsView() : await waitAgents(Number(a.timeout ?? 600))) };
    }
    turn.status = 'running';
    return { ...answeredNote, ...(a['no-wait'] ? await snapshot(turn) : await settle(turn, Number(a.timeout ?? 600))) };
  },
  cancel: async () => {
    if (!turn || turn.finalized) throw new Error('No turn is going');
    if (turn.spoken) await invoke('assistant_stop', {});
    else await main().getByRole('button', { name: /^Stop/ }).first().click({ timeout: 5_000 });
    return settle(turn, 30);
  },
  shot,
  state,
  logs: async (a) => logs(a),
  report: async (a) => {
    const { name, file } = promptFile(a._[0]);
    const text = fs.readFileSync(file, 'utf8');
    const cards = a.cards === 'allow' ? 'allow' : 'deny';
    const t = await startTurn({ text, chat: a.chat ?? 'new', autoCards: cards, report: { name, file }, ...shotsFor(a) });
    const out = a['no-wait'] ? await snapshot(t) : await settle(t, Number(a.timeout ?? 540));
    if (!t.finalized) out.hint = 'The report is still being written; the lab saves it when the turn ends: npm run lab -- wait';
    return out;
  },
  fake: async (a) => {
    if (!fakeLlm) throw new Error('Only with start --fake (and a model that isn’t the plan)');
    // (Placeholders — a ref on a live page, a point of a shot — filled in as each call is made: fake.mjs.)
    if (a.script) fakeLlm.scriptChat(templated(JSON.parse(a.script)));
    return { requests: fakeLlm.requests.length, chatCalls: fakeLlm.requests.filter((r) => r.tools?.length).length };
  },
  stop: async () => {
    setTimeout(() => shutdown('lab stop'), 50);
    return { stopped: true, dataDir: yap?.dataDir, lab: LAB };
  },
};

function serve(token) {
  const server = http.createServer((req, res) => {
    const reply = (status, body) => {
      res.writeHead(status, { 'content-type': 'application/json' });
      res.end(JSON.stringify(body));
    };
    const host = String(req.headers.host ?? '');
    if (req.method !== 'POST' || req.url !== '/cmd' || req.headers.origin || !/^127\.0\.0\.1:\d+$/.test(host) || req.headers.authorization !== `Bearer ${token}`) {
      reply(403, { ok: false, error: 'forbidden' });
      return;
    }
    let raw = '';
    req.on('data', (c) => (raw += c));
    req.on('end', async () => {
      lastCommand = Date.now();
      let body;
      try {
        body = JSON.parse(raw || '{}');
      } catch {
        reply(400, { ok: false, error: 'not JSON' });
        return;
      }
      const fn = COMMANDS[body.cmd];
      if (!fn) return reply(404, { ok: false, error: `No command "${body.cmd}"` });
      if (!yap && body.cmd !== 'ping') return reply(409, { ok: false, error: `The lab isn't ready (${control.state})` });
      try {
        const out = await fn(body.args ?? { _: [] });
        lastCommand = Date.now();
        reply(200, { ok: true, ...out });
      } catch (e) {
        log(`${body.cmd} failed:`, e.stack || e.message);
        reply(200, { ok: false, error: e.message.split('\n')[0] });
      }
    });
  });
  server.requestTimeout = 0;
  server.headersTimeout = 60_000;
  server.keepAliveTimeout = 5_000;
  return new Promise((resolve) => server.listen(0, '127.0.0.1', () => resolve(server.address().port)));
}

// ---------------------------------------------------------------- start

async function start() {
  const token = crypto.randomBytes(32).toString('hex');
  const port = await serve(token);
  writeControl({ state: 'starting', port, token, lab: LAB });
  const model = parseModel(START.modelSpec, { fake: !!START.fake });
  START.model = model;
  // A new run: the last one's turns and their shots go (its numbers start at 1
  // again); reports, sheets and `shot`'s own files stay.
  fs.rmSync(TURNS, { recursive: true, force: true });
  if (fs.existsSync(SHOTS)) {
    for (const name of fs.readdirSync(SHOTS)) if (/^\d{3,}$/.test(name)) fs.rmSync(path.join(SHOTS, name), { recursive: true, force: true });
  }
  // The main window's size (shell/lab.rs): the same in every run, so shots compare.
  const env = { YAP_LAB: '1', YAP_LAB_RECORD: RECORD, YAP_LAB_WINDOW: START.size ?? '1280x860' };
  // The real web and the person's own Claude Code — but with `--fake`, the
  // stand-ins (`--fake --live-web`: the fake AI on the real web, for the
  // lab's own checks; a stand-in for Claude Code still wins: YAP_CLAUDE_EXE).
  if (!START.fake || START.liveWeb) env.YAP_LAB_LIVE = '1';
  let fakeBase = '';
  if (START.fake) {
    if (!START.liveWeb) {
      const { startFakeWeb } = await harness('fake-web.js');
      fakeWeb = await startFakeWeb();
      Object.assign(env, fakeWeb.env);
    }
    if (model.kind === 'plan') {
      const script = START.fakeClaudeScript ?? path.join(APP, 'e2e', 'support', 'fake-claude', 'plan-chat.jsonl');
      Object.assign(env, { YAP_CLAUDE_EXE: START.exe, YAP_CLAUDE_ARGS_PREFIX: JSON.stringify(['fake-claude', script]) });
    } else {
      // Yap Code's agents: the stand-in for Claude Code, on an agent's script
      // (an edit, a permission prompt, a report).
      if (START.yapCode) {
        const script = START.fakeClaudeScript ?? path.join(APP, 'src-tauri', 'src', 'ai', 'claude_code', 'fixtures', 'fake', 'agent-hook-and-approve.jsonl');
        Object.assign(env, { YAP_CLAUDE_EXE: START.exe, YAP_CLAUDE_ARGS_PREFIX: JSON.stringify(['fake-claude', script]) });
      }
      const { startFakeLlm } = await harness('fake-llm.js');
      fakeLlm = await startFakeLlm();
      // The strong fake's server says its context, as a strong model's does.
      fakeLlm.setServerModels([{ id: 'fake-chat' }, { id: model.model }]);
      fakeLlm.serveOllama({ loaded: { [model.model]: 32768 } });
      fakeBase = fakeLlm.base;
    }
  }
  const config = labConfig({ model, effort: START.effort, web: START.web, yapCode: START.yapCode, connectors: START.connectors, key: process.env.YAP_LAB_ANTHROPIC_KEY ?? '', fakeBase });
  fs.rmSync(RECORD, { force: true });
  const { launchYap } = await harness('yap.js');
  log('launching the test Yap', START.exe);
  yap = await launchYap({ name: 'lab', runsDir: RUNS, exe: START.exe, config, env });
  yap.child.on('exit', (code) => {
    if (!stopping) {
      log(`the test Yap exited (${code})`);
      shutdown(`the test Yap exited (${code})`, 'exited');
    }
  });
  await tryInvoke('e2e_main_on_screen', {});
  // The main window at its size (set at the end of setup): its CSS pixels and scale.
  const [wantW, wantH] = String(START.size ?? '1280x860').split(/[x×]/i).map(Number);
  const size = () => main().evaluate(() => ({ width: window.innerWidth, height: window.innerHeight, dpr: window.devicePixelRatio || 1 }));
  await waitFor(async () => {
    const s = await size();
    return Math.abs(s.width - wantW) <= 1 && Math.abs(s.height - wantH) <= 1;
  }, 5_000, 'the main window’s size').catch(() => log('the main window isn’t at the size asked for'));
  const out = { model, dataDir: yap.dataDir, windows: Object.keys(yap.pages), window: await size().catch(() => null), yapPid: yap.child.pid };
  // Claude Code, as Yap checks it (found, its version, signed in): never a sign-in.
  if (model.kind === 'plan' || START.yapCode) {
    const status = await invoke('claude_plan_status', { refresh: true });
    const code = status?.code ?? status ?? {};
    out.claudeCode = { installed: !!code.installed, version: code.version ?? null, tooOld: !!code.tooOld, signedIn: !!code.loggedIn, plan: code.plan ?? null, authMethod: code.authMethod ?? null, error: code.error ?? null, standIn: !!START.fake };
    const problem = !code.installed
      ? "Claude Code isn't installed on this PC (Yap looks in ~/.local/bin and PATH)."
      : code.tooOld
        ? `Claude Code ${code.version} is too old for Yap (it needs ${code.minVersion ?? 'a newer one'}).`
        : !code.loggedIn
          ? "Claude Code isn't signed in on this PC. Sign in yourself (`claude auth login` in a terminal): the lab never signs in."
          : null;
    if (problem) throw Object.assign(new Error(problem), { out });
  }
  if (START.yapCode) {
    const dir = START.workspace ? path.resolve(START.workspace) : makeWorkspace(path.join(LAB, 'workspace', WORKSPACE_NAME), { keep: !!START.keepWorkspace });
    const added = await invoke('workspaces_add', { path: dir });
    workspace = { name: added?.name ?? path.basename(dir), path: dir };
    out.workspace = workspace;
  }
  writeControl({ state: 'ready', yapPid: yap.child.pid, dataDir: yap.dataDir, runDir: yap.runDir, result: out });
  log('ready');
  // Idle: quit after `--idle` minutes with no command and no turn going.
  const idleMs = Number(START.idle ?? 120) * 60_000;
  setInterval(() => {
    if (Date.now() - lastCommand > idleMs && (!turn || turn.finalized)) shutdown(`idle for ${START.idle ?? 120} minutes`);
  }, 30_000).unref();
  // Keep the record read and the turn watched between commands (a report's
  // turn ends, and is saved, with nobody waiting on it).
  setInterval(() => tick().catch((e) => log('tick failed', e.message)), 1000);
}

process.on('unhandledRejection', (e) => log('unhandled', e?.stack || e));
start().catch(async (e) => {
  log('start failed', e.stack || e.message);
  writeControl({ state: 'failed', error: e.message.split('\n')[0], result: e.out ?? null });
  await shutdown('start failed', 'failed');
});
