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
import { execFileSync } from 'node:child_process';
import crypto from 'node:crypto';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { APP, labConfig, localServer, minutesFrom, onOff, parseModel } from './options.mjs';
import { drawPng, pngSize } from './png.mjs';
import { templated } from './fake.mjs';
import { captureElement, captureLivePage, capturePanel, captureThread, captureWindow, livePage, pinTheme, shotNames, slug, themeNow, themesOf } from './shots.mjs';
import { labBrowser, makeSheet } from './sheet.mjs';
import { branchFrom, conversationOf, endOf, cardsOf, pendingOf, routineEnd, summarize, toolsOf, cut, planCommentsOf } from './turns.mjs';
import { makeGitRepo, makeWorkspace, NAME as WORKSPACE_NAME } from './workspace.mjs';

const LAB = path.resolve(process.argv[2] ?? '');
const START = JSON.parse(fs.readFileSync(path.join(LAB, 'start.json'), 'utf8'));
const CONTROL = path.join(LAB, 'control.json');
const RECORD = path.join(LAB, 'record.jsonl');
// Computer use's stand-in Start menu (apps/index.rs: test mode reads YAP_E2E_APPS at each look): written by
// `set --computer on`, empty (no file) until then.
const START_MENU = path.join(LAB, 'computer-start-menu.json');
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

/** How long a Code chat's session's answer may take to land once its turn is over (its report, chat/turn/report.rs). */
const SESSION_GRACE_MS = 20_000;

/** The turn going now, or the last one. */
let turn = null;
/** The test Yap told the person is in another app (`ask --spoken --away`). */
let awayNow = false;
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
  // (A turn of the lab's own ends when its command does: `actionTurn`.)
  if (!t || t.finalized || t.finalizing || t.ticking || t.kind === 'lab') return;
  t.ticking = true;
  try {
    // A routine's run (`routine new|now`): over when Settings' runs list says so, never by a Chat stream.
    if (t.routine) {
      const end = routineEnd(await tryInvoke('routines_list', {}, null), { id: t.routine.id, after: t.routine.after });
      if (end.run) t.run = end.run;
      if (!end.done) return;
      t.status = end.status;
      t.error = end.error;
      t.conversation = end.run.chat || end.run.thread || null;
      // Its question: the run's own message in that chat (`/<command> …`), the answer after it.
      const conv = t.conversation ? await tryInvoke('chat_get', { id: t.conversation }) : null;
      const from = branchFrom(conv);
      if (from) {
        t.askedAt = from.index;
        t.question = from.text || t.question;
      }
      t.endedAt = Date.now();
      await finalize(t);
      return;
    }
    t.conversation ??= conversationOf(t.items);
    const end = endOf(t.items, { spoken: t.spoken });
    // A Code chat's own session (turns.mjs directOf): over with its answer in the chat — or, its turn over and
    // no answer SESSION_GRACE_MS on, over anyway.
    const late = !end.done && end.session && end.ended && Date.now() - end.ended > SESSION_GRACE_MS;
    if (end.done || late) {
      t.status = end.done ? end.status : end.endedStatus === 'done' ? 'answered' : end.endedStatus;
      t.error = end.error;
      t.endedAt = Date.now();
      await finalize(t);
      return;
    }
    t.approvals = (await tryInvoke('claude_approve_pending', {}, [])) ?? [];
    // A plan an agent was already waiting with before this question isn't
    // this turn's card (`wait --agents --cards` answers it): only the ones
    // ready since. (A turn that starts an agent sets the older ones aside:
    // "Plan not used".)
    const agents = (await agentsNow()).filter((a) => !a.plan || (Number(a.endedTs) || 0) * 1000 >= t.startedAt);
    const pending = pendingOf(cardsOf(t.items, { approvals: t.approvals, agents })).filter((c) => !t.answered.has(c.id));
    // (Ended meanwhile — a question that couldn't be asked: how it ended stands.)
    if (t.finalizing) return;
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
  // (An Edit says so; a routine's turn has its run: when, why, where its answer went.)
  if (t.branch) out.branch = t.branch;
  if (t.routine) {
    out.run = runOf(t.run);
    // (A routine that had a chat already: did this run go to the same one?)
    if (out.run && t.routine.chatBefore) out.run.sameChat = out.run.chat === t.routine.chatBefore;
  }
  out.shots = t.shots.map(shotEntry);
  if (t.shotErrors.length) out.shotErrors = [...t.shotErrors];
  return out;
}

async function finalize(t) {
  // (How it ended, kept: a look at the turn already under way (`tick`) mustn't make a question that couldn't be
  // asked "running" again — 16 Nov 2026, "Couldn't ask" came back as a turn still going, for good.)
  t.finalizing = true;
  const ended = t.status;
  // Its last events (a card's end, the page's chips) a moment after its end.
  await sleep(300);
  t.status = ended;
  readRecord();
  const conv = t.conversation != null ? await tryInvoke('chat_get', { id: t.conversation }) : null;
  const pages = (await tryInvoke('e2e_page_views', {}, [])) ?? [];
  const agents = await agentsNow();
  fs.mkdirSync(TURNS, { recursive: true });
  const transcript = path.join(TURNS, `${String(t.n).padStart(3, '0')}.json`);
  const result = summarize({ turn: t, items: t.items, conv, pages, approvals: [], agents, transcript });
  if (t.autoAnswered.length) result.autoAnswered = t.autoAnswered;
  t.status = result.status;
  // Shots of its own first (a routine's notice on the bar, before its chat is opened and the card goes).
  for (const pre of t.shotsOn ? (t.preShots ?? []) : []) {
    if (pre.before) await pre.before().catch((e) => t.shotErrors.push(`${pre.set}: ${e.message.split('\n')[0]}`));
    await shotSet(t, pre).catch((e) => t.shotErrors.push(`${pre.set} shots: ${e.message.split('\n')[0]}`));
  }
  // (A routine run with the person away: back in Yap now, before its chat is opened.)
  if (t.away) await tryInvoke('e2e_focus', { focused: true });
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
    JSON.stringify({ turn: t.n, question: t.question, status: t.result?.status ?? t.status, conversation: t.conversation, spoken: !!t.spoken, ...(t.kind ? { kind: t.kind } : {}), shots: t.shots }, null, 2)
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
    // (A turn of the lab's own may say which windows, and how to bring them up: a routine's note, an action's view.)
    let list = windows ?? t?.windows ?? null;
    if (!windows && t?.windows && t.show) await t.show(t);
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
  return { askedAt: conv?.messages?.length ?? 0, conv };
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
  if (shown) return { askedAt: conv?.messages?.length ?? 0, conv };
  return openChat(String(id));
}

/** `--shots` / `--no-shots` and `--theme` for a turn: the lab's defaults (`start`) unless the command says. */
function shotsFor(a) {
  const on = a['no-shots'] ? false : a.shots !== undefined ? onOff(a.shots, true, 'shots') : START.shots !== false;
  return { shots: on, themes: themesOf(a.theme ?? START.theme) };
}

/**
 * Made-up pictures (`drawPng`'s options each) dropped on Chat, as a person
 * drops a picture file: they wait in the message box and go with the next
 * message. Nothing from outside: each is drawn here.
 */
async function dropPictures(pictures) {
  const dir = path.join(LAB, 'to-drop');
  fs.mkdirSync(dir, { recursive: true });
  const stamp = Date.now();
  const paths = pictures.map((p, i) => {
    const file = path.join(dir, `${stamp}-${i + 1}.png`);
    fs.writeFileSync(file, drawPng(p ?? {}));
    return file;
  });
  await invoke('e2e_drag_drop', { paths });
  await main().locator('[data-testid="chat-attachments"] .shotchip').nth(paths.length - 1).waitFor({ state: 'visible', timeout: 15_000 });
}

/**
 * Files dropped on Chat (`--attach`, 15 Nov 2026: chat/attachments.rs), as a person drops them: each read by Yap and
 * waiting in the message box as a chip, going with the next message. Paths on this PC (the lab's own, or the scratch
 * workspace's made-up files).
 */
async function dropFiles(files) {
  const paths = files.map((f) => path.resolve(String(f)));
  for (const p of paths) if (!fs.existsSync(p)) throw new Error(`No file at ${p}`);
  const chips = main().locator('[data-testid="chat-files"] [data-testid="chat-file-chip"]');
  const before = await chips.count();
  await invoke('e2e_drag_drop', { paths });
  await chips.nth(before + paths.length - 1).waitFor({ state: 'visible', timeout: 60_000 });
}

async function startTurn({ text, spoken = false, away = false, chat = 'new', autoCards = null, report = null, shots = true, themes, pictures = null, attach = null, how = 'ask', style = null, incognito = false, project = null, research = false }) {
  if (!text?.trim()) throw new Error('Nothing to ask');
  if (turn && !turn.finalized) throw new Error(`Turn ${turn.n} is still going (${turn.status}): wait for it, answer its card, or cancel it`);
  // `--away` (a spoken question asked from another app, 16 Nov 2026): the test Yap told its window isn't in front
  // (e2e_focus, as the e2e suite's `inAnotherApp`), so the answer comes on the Yap bar's card. Left so until the next
  // question asked without it, so `shot --window bar` still finds the card.
  if (away !== awayNow) {
    await invoke('e2e_focus', { focused: !away });
    awayNow = away;
  }
  readRecord();
  const since = lastSeq();
  const opened = await (chat === 'same' ? sameChat() : openChat(chat)).catch((e) => {
    throw new Error(`Couldn't open Chat on ${chat || 'a new chat'}: ${e.message.split('\n')[0]}`);
  });
  let { askedAt } = opened;
  const question = text;
  // A new version starts at the person's message it replaces (branchFrom): a Code chat's Edit rewinds the thread
  // there and asks again, so the conversation's length when asked found no answer (16 Nov 2026: `answer` came back
  // empty though the thread had it).
  if (how === 'edit') {
    const from = branchFrom(opened.conv);
    if (from) askedAt = from.index;
  }
  const t = new Turn({ question, spoken, since, askedAt, autoCards, report, shots, themes });
  if (how !== 'ask') t.branch = how;
  turn = t;
  try {
    // A new chat started from a project's page (chat/projects.rs), the project made first if need be.
    if (project) await newChatInProject(project);
    // The chat's style from Style in the box's + menu (chat/styles.rs), as the person picks it.
    if (style) await pickStyle(style);
    // Research from the box's + menu (chat/research.rs; 16 Nov 2026), switched on as the person would: its chip in
    // the box says it's on.
    if (research) {
      const chip = main().getByTestId('chat-research-on');
      if (!(await chip.count())) {
        await main().getByRole('button', { name: 'Chat options' }).click({ timeout: 10_000 });
        const row = main().getByTestId('chat-plus-research');
        await row.click({ timeout: 10_000 });
        // (The row asks Rust whether Chat's model can research before it turns on: on a moment after the click.)
        const on = await chip
          .waitFor({ state: 'visible', timeout: 5_000 })
          .then(() => true)
          .catch(() => false);
        if (!on) {
          const why = await row.getAttribute('title').catch(() => '');
          await main().keyboard.press('Escape').catch(() => {});
          throw new Error(`Research can't be switched on: ${why}`);
        }
      }
    }
    // A new chat made incognito with its ghost (chat/incognito.rs).
    if (incognito) {
      const ghost = main().getByTestId('chat-incognito');
      if ((await ghost.getAttribute('aria-pressed', { timeout: 10_000 })) !== 'true') await ghost.click({ timeout: 10_000 });
    }
    if (how === 'edit') {
      await editTurn(text);
    } else if (spoken) {
      // (Pictures and files in the box first, as a person drops them before asking out loud: the question takes
      // them with Chat in view, 16 Nov 2026.)
      if (pictures?.length) await dropPictures(pictures);
      if (attach?.length) await dropFiles(attach);
      await invoke('e2e_assistant_ask', { text, listenMs: 300 });
    } else {
      if (pictures?.length) await dropPictures(pictures);
      if (attach?.length) await dropFiles(attach);
      const box = main().getByRole('textbox', { name: 'Message' });
      await box.fill(text, { timeout: 10_000 });
      // (Toasts never cover the box since 16 Nov 2026, ui/toastClear.js: Send is clicked as the person clicks it.)
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

/**
 * **Edit** the person's last message to `text` (15 Nov 2026, the Claude app's message actions:
 * chat/chats/branches.rs), as the person does — by its buttons: the pencil, the words in place, Save and send. A new
 * version of the conversation from there (‹ 2/2 ›). Chat's only: a Code chat has no Edit (16 Nov 2026).
 */
async function editTurn(text) {
  const col = main().locator('[data-testid="chat-column"]');
  await col.locator('.cbubble.user').last().hover({ timeout: 10_000 });
  await col.getByTestId('chat-edit-message').last().click({ timeout: 10_000 });
  await col.getByTestId('chat-edit-input').fill(text, { timeout: 10_000 });
  await col.getByTestId('chat-edit-save').click({ timeout: 10_000 });
}

/**
 * **A new chat in a project** (`--project`, chat/projects.rs): the project named `project.name` made if there's none
 * (its instructions set when given), then its page opened and its New chat pressed, as the person does — or, with
 * `project.page` (`--project-page`, 16 Nov 2026), its page left in view (a question asked out loud there starts its
 * new chat in it: chat/assistant.rs `new_in_project`).
 */
async function newChatInProject(project) {
  const name = String(project.name).trim();
  let p = ((await invoke('projects_list')) || []).find((x) => x.name === name);
  if (!p) p = await invoke('project_create', { name, description: null });
  if (project.instructions) await invoke('project_update', { id: p.id, instructions: String(project.instructions) });
  // Its files (`--project-files`), each added once (by its name), as its page's Add files… adds them.
  if (project.files?.length) {
    const paths = project.files.map((f) => path.resolve(String(f)));
    for (const f of paths) if (!fs.existsSync(f)) throw new Error(`No file at ${f}`);
    const have = new Set(((await invoke('project_get', { id: p.id }))?.project?.files || []).map((f) => f.name));
    const add = paths.filter((f) => !have.has(path.basename(f)));
    if (add.length) {
      const out = await invoke('project_files_add', { id: p.id, paths: add });
      if (out?.skipped?.length) throw new Error(`Yap didn't add ${out.skipped.map((s) => `${s.name} (${s.why})`).join(', ')}`);
    }
  }
  const m = main();
  await m.getByRole('button', { name: 'Projects', exact: true }).click({ timeout: 10_000 });
  await m.getByTestId('chat-projects').getByRole('button', { name }).first().click({ timeout: 10_000 });
  if (project.page) {
    await m.getByTestId('chat-project').getByRole('button', { name: `New chat in ${name}` }).waitFor({ state: 'visible', timeout: 10_000 });
    return;
  }
  await m.getByRole('button', { name: `New chat in ${name}` }).click({ timeout: 10_000 });
}

/** `--project-files a,b` (or a script step's list): the paths, or null. */
function projectFiles(v) {
  if (Array.isArray(v)) return v.map(String).filter(Boolean);
  return typeof v === 'string' && v.trim() ? v.split(',').map((s) => s.trim()).filter(Boolean) : null;
}

/** Style from the box's + menu (chat/styles.rs; Style ›): the one named `name` (any case), as the person picks it. */
async function pickStyle(name) {
  const m = main();
  await m.getByRole('button', { name: 'Chat options' }).click({ timeout: 10_000 });
  await m.getByTestId('chat-plus-style').click({ timeout: 10_000 });
  const wanted = String(name).trim().toLowerCase();
  const items = m.getByTestId('chat-plus-style-menu').getByRole('menuitemradio');
  // (The styles are read as the box shows: wait for them.)
  await items.first().waitFor({ state: 'visible', timeout: 10_000 });
  const names = (await items.allTextContents()).map((t) => t.trim().toLowerCase());
  const at = names.indexOf(wanted);
  if (at < 0) throw new Error(`No style called "${name}" (there are: ${names.join(', ')})`);
  const item = items.nth(at);
  await item.click({ timeout: 10_000 });
}

// ---------------------------------------------------------------- cards

/**
 * **A question card of Chat's own model** (chat/cards/ask.rs) answered as the person would, on the card in the
 * thread when it's there: `picks` one entry a question (its labels joined with ","), `null` Let Yap decide.
 * A single question with one answer: its option clicked; otherwise each picked, then Send.
 */
async function answerQuestion(card, picks) {
  const id = Number(card.id.split(':')[1]);
  const box = main().locator(`[data-testid="chat-question-card"][data-state="asking"]`).first();
  const onScreen = await box.isVisible().catch(() => false);
  if (picks == null) {
    if (onScreen) {
      await box.getByRole('button', { name: 'Let Yap decide' }).click({ timeout: 5_000 });
      return true;
    }
    return invoke('chat_question_answer', { id, answers: null });
  }
  const qs = card.questions ?? [];
  const answers = Object.fromEntries(qs.map((q, i) => [q.question, q.multiSelect ? String(picks[i] ?? '').split(',').map((s) => s.trim()).filter(Boolean) : String(picks[i] ?? '').trim()]).filter(([, a]) => (Array.isArray(a) ? a.length : a)));
  if (onScreen && qs.length === 1 && !qs[0].multiSelect && qs[0].options.includes(answers[qs[0].question])) {
    await box.getByTestId('chat-question-option').filter({ has: main().locator('.qlabel', { hasText: answers[qs[0].question] }) }).first().click({ timeout: 5_000 });
    return true;
  }
  return invoke('chat_question_answer', { id, answers });
}

/** The question card (chat/cards/ask.rs) or document outline (chat/cards/outline.rs) the current turn waits on, if one. */
async function waitingQuestion() {
  readRecord();
  const cards = cardsOf(turn?.items ?? [], { approvals: [], agents: [] });
  return pendingOf(cards).find((c) => (c.kind === 'question' || c.kind === 'outline') && !turn.answered.has(c.id)) ?? null;
}

/**
 * **A document's outline answered as the person would** (chat/cards/outline.rs): `edit` null — Go, clicked on the card
 * when it's there; else `{sections: [{key, intent}] | ["Heading", …], note}` sent as Edit's Go would.
 */
async function answerOutline(card, edit) {
  const id = Number(card.id.split(':')[1]);
  if (edit == null) {
    const box = main().locator(`[data-testid="chat-outline-card"][data-state="asking"]`).first();
    if (await box.isVisible().catch(() => false)) {
      // (A research's plan, chat/research.rs: its Go is "Start research".)
      await box.getByRole('button', { name: /^(Go|Start research)$/ }).click({ timeout: 5_000 });
      return true;
    }
    return invoke('chat_outline_answer', { id, outline: null });
  }
  const sections = (edit.sections ?? []).map((s) => (typeof s === 'string' ? { key: s, intent: '' } : { key: s.key ?? '', intent: s.intent ?? '' }));
  return invoke('chat_outline_answer', { id, outline: { sections, note: String(edit.note ?? '') } });
}


/** Answer `card` as the person would: "allow" (Allow once / Go ahead), "deny", or "always" (a connector's Always allow). */
async function answerCard(card, how, pick) {
  const [kind, raw] = card.id.split(/:(.*)/s);
  const allow = how !== 'deny';
  let answered;
  if (kind === 'confirm' && card.kind === 'outline' && how !== 'said') {
    // Go ("allow"), or Edit's Go with the sections and note given ("edit '<json>'").
    answered = await answerOutline(card, how === 'edit' ? JSON.parse(String(pick ?? '{}')) : null);
  } else if (kind === 'approve' && how === 'revise') {
    // A plan sent back with comments (claude_live/approve.rs): Claude Code stays in Plan mode and answers with a new plan.
    answered = await invoke('claude_approve_revise', { id: Number(raw), comments: planCommentsOf(pick) });
  } else if (kind === 'confirm' && how === 'said') {
    // Out loud: the Voice Assistant's next question in that chat, its words the answer (spoken_ok.rs).
    await invoke('e2e_assistant_ask', { text: String(pick ?? ''), listenMs: 300 });
    answered = true;
  } else if (kind === 'confirm' && card.kind === 'question') {
    // "allow": each question's first option; "pick": the labels given; "deny": Let Yap decide.
    const picks = how === 'deny' ? null : how === 'pick' ? String(pick ?? '').split('|') : (card.questions ?? []).map((q) => q.options?.[0] ?? '');
    answered = await answerQuestion(card, picks);
  } else if (kind === 'confirm') {
    answered = card.kind === 'connector'
      ? await invoke('chat_connector_confirm', { id: Number(raw), answer: how === 'always' ? 'always' : allow ? 'once' : 'deny' })
      : await invoke('chat_page_confirm', { id: Number(raw), allow });
  } else if (kind === 'approve' && allow && card.questions?.length) {
    // A question of Claude Code's own (AskUserQuestion): "allow" picks each question's first option; "deny" lets it decide.
    const answers = Object.fromEntries(card.questions.map((q) => [q.question, q.options?.[0]?.label ?? '']));
    answered = await invoke('claude_approve_reply', { id: Number(raw), answers });
  } else if (kind === 'approve') {
    answered = await invoke('claude_approve_answer', { id: Number(raw), allow });
  } else if (kind === 'plan') {
    // Go ahead, or Not yet — the Agent panel's: the plan set aside ("Plan not
    // used", agents/runner.rs `move_to_finished`), so it waits no more. (Stop
    // did nothing to an agent whose turn is over.)
    const latest = (await agentsNow()).find((a) => a.id === raw)?.turnList?.at(-1)?.n ?? 1;
    answered = !!(allow ? await invoke('agent_go_ahead', { id: raw }) : await invoke('agent_move_to_finished', { id: raw, turn: latest }));
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
  const calls = toolsOf(t.items).map((tool, i) => `${i + 1}. \`${tool.name}\`${tool.via === 'claude' && tool.server ? ` (${tool.server})` : ''}${tool.via === 'agent' ? ' (an agent at work, not this turn)' : ''} ${tool.ok === false ? (tool.refused ? '**refused**' : '**failed**') : 'ok'}: ${cut(String(tool.args ?? ''), 160).replace(/\n/g, ' ')} → ${cut(String(tool.result ?? ''), 240).replace(/\n/g, ' ')}`);
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
async function probe({ window: which = 'main', css, text, at }) {
  if (!css && !text) throw new Error('probe needs --css or --text');
  if (at != null && !AT_POINTS.includes(at)) throw new Error(`--at takes ${AT_POINTS.join(' or ')}`);
  const views = (await tryInvoke('e2e_page_views', {}, [])) ?? [];
  const page = which === 'bar' ? yap.overlay : which === 'page' ? livePage(yap, views) : main();
  if (!page) throw new Error('No page beside the chat');
  const scope = which === 'panel' ? '[data-testid="chat-panel-slot"]' : which === 'thread' ? '[data-testid="chat-column"]' : null;
  const got = await page.evaluate(
    ([scope, css, text, at]) => {
      const root = scope ? document.querySelector(scope) : document.body;
      if (!root) return { found: false, count: 0, visible: 0, ...(text ? { text: false } : {}) };
      const out = { found: true, theme: document.documentElement.dataset.yapTheme ?? null };
      if (css) {
        const els = [...root.querySelectorAll(css)];
        out.count = els.length;
        const shows = els.filter((e) => e.getClientRects().length > 0 && getComputedStyle(e).visibility !== 'hidden');
        out.visible = shows.length;
        // Whether something else is over the first that shows, at its centre (16 Nov 2026: a toast over Send), and what;
        // `at` (16 Nov 2026: a file link's ⋯ over its end) its last line's end, or its first line's start, 2 px in.
        if (shows.length) {
          const r = shows[0].getBoundingClientRect();
          const lines = [...shows[0].getClientRects()].filter((l) => l.width > 0.5);
          const line = at === 'end' ? lines.at(-1) : at === 'start' ? lines[0] : null;
          const x = line ? (at === 'end' ? line.right - 2 : line.left + 2) : r.left + r.width / 2;
          const y = line ? (line.top + line.bottom) / 2 : r.top + r.height / 2;
          if (at) out.at = at;
          const top = document.elementFromPoint(x, y);
          out.covered = !!top && !shows[0].contains(top) && !top.contains(shows[0]);
          if (out.covered) {
            const toast = top.closest('[role="status"]');
            const named = (e) => `${e.tagName.toLowerCase()}${e.getAttribute('data-testid') ? `[data-testid="${e.getAttribute('data-testid')}"]` : e.className && typeof e.className === 'string' ? `.${e.className.trim().split(/\s+/)[0]}` : ''}`;
            out.over = toast ? `a toast: ${String(toast.innerText || '').replace(/\s+/g, ' ').trim().slice(0, 80)}` : named(top);
          }
        }
      }
      if (text) {
        const norm = (s) => String(s ?? '').replace(/\s+/g, ' ').trim().toLowerCase();
        out.text = norm(root.innerText).includes(norm(text));
      }
      return out;
    },
    [scope, css ?? null, text ?? null, at ?? null]
  );
  const { text: has, ...rest } = got;
  return { window: which, ...(css ? { css } : {}), ...rest, ...(text ? { words: cut(text, 120), has } : {}) };
}

/**
 * **Clicks a button of the test Yap's own** (`--css`, in `--window` main / thread / panel), as the person would
 * (14 Nov 2026, the message box's queue: a queued message's Send now and Cancel and edit). Only Yap's own windows,
 * never a page beside the chat (that's Claude's to act on, with its rules); the first match that shows.
 */
/**
 * **What the test Yap's voice was given** (16 Nov 2026, voice/tts/maths.rs): test mode plays nothing, so this is
 * the words each spoken reply or read-aloud was handed (`tts_e2e_spoken`: its sentences, engine, message id), and
 * the reading card as it is now (`read_aloud_state`: its sentence as shown, the word lit, how it was timed).
 * `--clear`: forgotten after reading them, so the next `spoken` shows only what's new.
 */
async function spokenCmd({ clear = false }) {
  const said = (await tryInvoke('tts_e2e_spoken', clear ? { clear: true } : {}, [])) ?? [];
  const r = await tryInvoke('read_aloud_state', {}, null);
  const parts = r?.parts ?? [];
  const reading = r?.active
    ? {
        phase: r.phase,
        index: r.index,
        count: r.count,
        shown: parts.map((p) => p.text).join(''),
        word: r.word ?? null,
        lit: parts.find((p) => p.w != null && p.w === r.word)?.text ?? null,
        timing: r.timing ?? null,
        messageId: r.messageId ?? null,
      }
    : null;
  return { spoken: said.map((s) => ({ messageId: s.messageId ?? null, engine: s.engine, streamed: !!s.streamed, sentences: s.sentences })), reading };
}

/**
 * **What the test Yap copied** (16 Nov 2026, the rows under Chat's messages: Copy): its page's clipboard writes are
 * held in the page from the start (`holdClipboard`), never put on this PC's clipboard — the person's own, in use
 * meanwhile. `--clear`: forgotten after reading them.
 */
async function copiedCmd({ clear = false }) {
  const copied = await main().evaluate((clear) => {
    const all = [...(window.__labCopied ?? [])];
    if (clear && window.__labCopied) window.__labCopied.length = 0;
    return all;
  }, clear);
  return { copied };
}

/** The page's clipboard writes kept in it (`copied`), now and after any reload: this PC's clipboard never touched. */
async function holdClipboard() {
  const hold = () => {
    if (window.__labCopied) return;
    window.__labCopied = [];
    navigator.clipboard.writeText = async (t) => {
      window.__labCopied.push(String(t));
    };
  };
  await main().addInitScript(hold).catch(() => {});
  await main().evaluate(hold).catch(() => {});
}

/** Where `click --at` and `probe --at` aim: a link's last line's end, or its first line's start (2 px in). */
const AT_POINTS = ['end', 'start'];

async function click({ window: which = 'main', css, right = false, hover = false, at }) {
  if (!css) throw new Error('click needs --css');
  if (!['main', 'thread', 'panel', 'bar'].includes(which)) throw new Error("click works in Yap's own windows: main, thread, panel or bar");
  if (at != null && !AT_POINTS.includes(at)) throw new Error(`--at takes ${AT_POINTS.join(' or ')}`);
  const scope = which === 'panel' ? '[data-testid="chat-panel-slot"] ' : which === 'thread' ? '[data-testid="chat-column"] ' : '';
  // `bar`: the Yap bar's own buttons (the reading card's ⏸, 16 Nov 2026).
  const target = (which === 'bar' ? yap.overlay : main()).locator(`${scope}${css}`).first();
  // `--at end|start` (16 Nov 2026: a short file link's end, once under its ⋯): that point, from the box's corner.
  let position;
  if (at) {
    await target.scrollIntoViewIfNeeded({ timeout: 5_000 });
    position = await target.evaluate((el, at) => {
      const box = el.getBoundingClientRect();
      const lines = [...el.getClientRects()].filter((l) => l.width > 0.5);
      const line = at === 'end' ? lines.at(-1) : lines[0];
      return { x: (at === 'end' ? line.right - 2 : line.left + 2) - box.left, y: (line.top + line.bottom) / 2 - box.top };
    }, at);
  }
  const where = position ? { position } : {};
  // `--hover`: the pointer put on it, no click (a file link's ⋯, 16 Nov 2026): what shows then, `shot` or `click` next.
  if (hover) {
    await target.hover({ timeout: 5_000, ...where });
    return { window: which, css, hovered: true, ...(at ? { at } : {}) };
  }
  // `--right`: a right-click (a file link's menu, 16 Nov 2026).
  await target.click({ timeout: 5_000, button: right ? 'right' : 'left', ...where });
  return { window: which, css, clicked: true, ...(right ? { right: true } : {}), ...(at ? { at } : {}) };
}

/**
 * **Types into the test Yap's own window** as the person would (16 Nov 2026, for the sheet view's cells): `--css`
 * clicked first (in `--window` main / thread / panel), then `--text` typed as real key presses, then each key of
 * `--press` (`Enter`, `Tab`, `Control+z`, comma-separated). Never a page beside the chat: that's Claude's to act on.
 */
async function typeKeys({ window: which = 'main', css, text, press }) {
  if (!['main', 'thread', 'panel'].includes(which)) throw new Error("type works in Yap's own window: main, thread or panel");
  if (!css && text == null && !press) throw new Error('type needs --css, --text or --press');
  const scope = which === 'panel' ? '[data-testid="chat-panel-slot"] ' : which === 'thread' ? '[data-testid="chat-column"] ' : '';
  if (css) await main().locator(`${scope}${css}`).first().click({ timeout: 5_000 });
  if (text != null && String(text) !== '') await main().keyboard.type(String(text), { delay: 25 });
  const keys = String(press ?? '').split(',').map((k) => k.trim()).filter(Boolean);
  for (const k of keys) await main().keyboard.press(k);
  return { window: which, ...(css ? { css } : {}), typed: text == null ? 0 : String(text).length, pressed: keys };
}

/**
 * **A side question** (Ctrl+;, chat/side.rs, 15 Nov 2026): asked about the chat shown, in the box over its thread, as
 * the person asks one — the key, the words, Enter — and its answer read back from the box. Nothing of it is in the
 * chat (the next `state` or `ask` shows so) unless `keep`: Keep it, once it can be pressed (the chat's own answer,
 * or its Claude's work, done first). `close`: the box closed after (Esc), as the person throws it away.
 */
async function side({ text, keep = false, close = false, timeout = 180 }) {
  const question = String(text ?? '').trim();
  if (!question) throw new Error('side needs a question');
  const m = main();
  const box = m.getByTestId('chat-sideq');
  if (!(await box.count())) await m.keyboard.press('Control+;');
  await box.waitFor({ state: 'visible', timeout: 10_000 });
  const input = m.getByTestId('chat-sideq-input');
  await input.fill(question);
  await input.press('Enter');
  const started = Date.now();
  const deadline = started + Number(timeout) * 1000;
  // Answered: Keep it shows (a whole answer), or an error does.
  let error = null;
  while (Date.now() < deadline) {
    if (await m.getByTestId('chat-sideq-keep').count()) break;
    error = await box.locator('.err').first().innerText({ timeout: 200 }).catch(() => null);
    if (error) break;
    await sleep(250);
  }
  const answer = await m.getByTestId('chat-sideq-answer').innerText({ timeout: 1_000 }).catch(() => '');
  const out = { question: cut(question, 200), answer: cut(answer, 2000), error, seconds: Math.round((Date.now() - started) / 100) / 10, conversation: turn?.conversation ?? null };
  if (keep && answer) {
    const btn = m.getByTestId('chat-sideq-keep');
    out.keptAfterWaiting = await btn.isDisabled().catch(() => false);
    while (Date.now() < deadline && (await btn.isDisabled().catch(() => false))) await sleep(500);
    await btn.click({ timeout: 5_000 });
    await box.waitFor({ state: 'detached', timeout: 10_000 });
    out.kept = true;
  } else if (close) {
    await input.press('Escape');
    await box.waitFor({ state: 'detached', timeout: 5_000 });
    out.closed = true;
  }
  return out;
}

/**
 * **Select an element on the page beside the chat**, as a person does (the page tab's arrow, Ctrl+Shift+S):
 * its tools open with Select on, the element showing `text` (the smallest that holds the words) — or `css` —
 * scrolled into view and clicked through DevTools' own inspect overlay (real input, `e2e_page_click`), and its
 * chip awaited in the message box. The next `ask` sends it with the words typed.
 */
async function pick({ text, css }) {
  if (!text && !css) throw new Error('pick needs words on the page (or --css)');
  const views = (await tryInvoke('e2e_page_views', {}, [])) ?? [];
  const front = views.find((v) => v.onScreen) ?? views[0];
  if (!front) throw new Error('No page beside the chat');
  const m = main();
  const chips = m.locator('[data-testid="chat-elements"] [data-testid="chat-element-chip"]');
  const before = await chips.count();
  if (!(await m.locator('[data-testid="chat-page-tools"]').count())) await m.locator('[data-testid="chat-page-select"]').click();
  await m.locator('[data-testid="chat-page-tools"]').waitFor({ timeout: 5000 });
  const find = `(() => {
    const css = ${JSON.stringify(css ?? null)}, want = ${JSON.stringify(String(text ?? '').replace(/\s+/g, ' ').trim().toLowerCase())};
    const shown = (e) => e.getClientRects().length > 0 && getComputedStyle(e).visibility !== 'hidden';
    let el = css ? document.querySelector(css) : null;
    if (!css) {
      let best = null;
      for (const e of document.body.querySelectorAll('*')) {
        if (!shown(e)) continue;
        const t = (e.innerText || '').replace(/\\s+/g, ' ').trim().toLowerCase();
        if (t.includes(want) && (!best || t.length < best.t)) best = { e, t: t.length };
      }
      el = best && best.e;
    }
    if (!el) return null;
    el.scrollIntoView({ block: 'center', inline: 'nearest', behavior: 'instant' });
    const r = el.getBoundingClientRect();
    return { x: r.left + Math.min(r.width / 2, 24), y: r.top + r.height / 2 };
  })()`;
  const at = JSON.parse(await invoke('e2e_page_view_eval', { key: front.key, script: find }));
  if (!at) throw new Error(css ? `Nothing on the page matches ${css}` : `Nothing on the page says “${cut(text, 60)}”`);
  await invoke('e2e_page_click', { key: front.key, x: at.x, y: at.y });
  await chips.nth(before).waitFor({ timeout: 20_000 });
  return { picked: (await chips.nth(before).locator('.eltext').textContent())?.trim() ?? '', page: front.url, chips: before + 1 };
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
  // The lab's read-only gh (agents/prs/lab.rs: gh pr list / view on Yap's own public repository, nothing else).
  if (a.gh !== undefined) {
    const on = onOff(a.gh, false, 'gh');
    if (on && START.fake) throw new Error('The read-only gh is for a live lab (start without --fake)');
    done.gh = { on, exe: await invoke('e2e_gh_lab', { on }) };
  }
  // The scratch workspace on a git branch, Yap's own repository its remote (no commit, nothing fetched).
  if (a.git !== undefined) {
    if (!workspace) throw new Error('No workspace to make a git repository of: start the lab with --yap-code on');
    if (!path.resolve(workspace.path).startsWith(path.resolve(LAB) + path.sep)) throw new Error(`set --git only works on the lab's scratch workspace, not ${workspace.path}`);
    const run = (cmd, args) => execFileSync(cmd, args, { encoding: 'utf8', windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'] });
    done.git = makeGitRepo(workspace.path, String(a.git), { run });
  }
  // The start screen's clock (ChatThread.svelte, 16 Nov 2026): a time of the lab's own for Chat's greeting — "23:40"
  // (today), "2026-11-18T23:40", or "now" (the real time again) — so a late night can be shot. Nothing else reads it.
  if (a.clock !== undefined) {
    const ms = clockAt(a.clock);
    await main().evaluate((v) => {
      window.__yapTestClock = v;
      window.dispatchEvent(new Event('yap-test-clock'));
    }, ms);
    done.clock = ms == null ? 'now' : new Date(ms).toString();
  }
  // The folder a Code chat's next Add folder takes (17 Nov 2026, the + menu: agents/menu.rs): test mode never opens
  // Windows' folder picker, so the lab names it (`e2e_workspace`). Yap's own rules still check it.
  const pick = a['folder-pick'] ?? a.folderPick;
  if (pick !== undefined) {
    const dir = path.resolve(String(pick));
    if (!fs.existsSync(dir) || !fs.statSync(dir).isDirectory()) throw new Error(`${dir} isn't a folder`);
    await invoke('e2e_workspace', { path: dir });
    done.folderPick = dir;
  }
  const files = a.files ?? (a.file ? [a.file] : []);
  if (files.length) {
    if (!workspace) throw new Error('No workspace to seed files in: start the lab with --yap-code on');
    // Only the lab's own scratch workspace: never a folder of the person's (`--workspace`).
    if (!path.resolve(workspace.path).startsWith(path.resolve(LAB) + path.sep)) throw new Error(`set only seeds the lab's scratch workspace, not ${workspace.path}`);
    done.files = files.map((f) => seedFile(workspace.path, f));
  }
  // Computer use (computer.rs, 19 Nov 2026) on a stand-in desktop: test windows drawn by the test Yap itself
  // (computer/sim.rs), never the real screen, mouse or keyboard; its Start menu a stand-in list of test apps.
  // `--computer real` (a lab started with --real-screen on, 20 Nov 2026): the person's real screen, mouse and
  // keyboard, Windows' own apps only on its Start menu; computer use's own grants and cards the only gate.
  if (a.computer === 'real') {
    if (!START.realScreen) throw new Error('--computer real needs a lab started with --real-screen on');
    fs.writeFileSync(START_MENU, JSON.stringify(REAL_START_MENU, null, 2));
    await invoke('e2e_computer_scene', { scene: null });
    await patchConfig({ computerUse: true, assistantScreen: true, assistantScreenCloud: true, assistantScreenAsked: true });
    done.computer = { apps: REAL_START_MENU.map((x) => x.name), screen: 'real' };
  } else if (a.computer !== undefined) {
    const on = onOff(a.computer, false, 'computer');
    if (on) {
      fs.writeFileSync(START_MENU, JSON.stringify(COMPUTER_START_MENU, null, 2));
      await invoke('e2e_computer_scene', { scene: COMPUTER_SCENE });
      await patchConfig({ computerUse: true, assistantScreen: true, assistantScreenCloud: true, assistantScreenAsked: true });
    } else {
      fs.rmSync(START_MENU, { force: true });
      await invoke('e2e_computer_scene', { scene: null });
      await patchConfig({ computerUse: false });
    }
    done.computer = on ? { apps: COMPUTER_START_MENU.map((a) => a.name), displays: COMPUTER_SCENE.displays.length, windows: COMPUTER_SCENE.windows.length } : 'off';
  }
  if (!Object.keys(done).length) throw new Error('Nothing to set: --web, --yap-code, --connectors, --bypass, --gh, --git, --clock, --folder-pick, --computer, config or files');
  return done;
}

/** `set --computer real`'s Start menu: Windows' own apps (their Start-menu ids), really started there. */
const REAL_START_MENU = [
  { id: 'Microsoft.WindowsNotepad_8wekyb3d8bbwe!App', name: 'Notepad' },
  { id: 'Microsoft.Paint_8wekyb3d8bbwe!App', name: 'Paint' },
  { id: 'Microsoft.WindowsAlarms_8wekyb3d8bbwe!App', name: 'Clock' },
  { id: 'Microsoft.Windows.Photos_8wekyb3d8bbwe!App', name: 'Photos' },
  { id: 'windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel', name: 'Settings' },
  { id: 'Microsoft.Windows.Explorer', name: 'File Explorer' },
];

/** `set --computer on`'s Start menu: test apps only (made-up paths; nothing is ever launched in test mode). */
const COMPUTER_START_MENU = [
  { id: 'Microsoft.WindowsNotepad_8wekyb3d8bbwe!App', name: 'Notepad' },
  { id: 'Microsoft.WindowsCalculator_8wekyb3d8bbwe!App', name: 'Calculator' },
  { id: 'Microsoft.Paint_8wekyb3d8bbwe!App', name: 'Paint' },
  { id: 'Microsoft.Windows.Explorer', name: 'File Explorer' },
  { id: 'MSEdge', name: 'Microsoft Edge', target: 'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe' },
  { id: 'Microsoft.WindowsTerminal_8wekyb3d8bbwe!App', name: 'Terminal' },
  { id: 'C:\\Program Files\\Vaultkeeper\\KeePass.exe', name: 'KeePass', target: 'C:\\Program Files\\Vaultkeeper\\KeePass.exe' },
  // Not on screen: open_application starts it (the stand-in desktop gets a window for it).
  { id: 'Microsoft.MicrosoftStickyNotes_8wekyb3d8bbwe!App', name: 'Sticky Notes' },
];

/** `set --computer on`'s desktop (computer/sim.rs): two displays, test windows topmost first, the desktop last.
 * Each app has things to click and type into (computer use's phase 2): Notepad a note, Save, Delete and a
 * link; Calculator its keys; a Terminal (clicks only); a sign-in box with a password field. */
const W = (kind, name, x, y, w = 220, h = 34, text = '') => ({ kind, name, x, y, w, h, text });
const COMPUTER_SCENE = {
  displays: [
    { x: 0, y: 0, w: 1920, h: 1080, primary: true },
    { x: 1920, y: 0, w: 1280, h: 720 },
  ],
  windows: [
    { x: 1250, y: 600, w: 560, h: 380, own: true, class: 'Tauri Window', title: 'Yap' },
    { x: 980, y: 140, w: 480, h: 300, exe: 'C:\\Program Files\\Example Mail\\mail.exe', class: 'MailWindow', title: 'Mail' },
    {
      x: 140,
      y: 90,
      w: 820,
      h: 560,
      exe: 'C:\\Program Files\\WindowsApps\\Microsoft.WindowsNotepad_11.2410.21.0_x64__8wekyb3d8bbwe\\Notepad\\Notepad.exe',
      aumid: 'microsoft.windowsnotepad_8wekyb3d8bbwe',
      class: 'Notepad',
      title: 'Shopping list - Notepad',
      widgets: [
        W('field', 'Note', 20, 50, 760, 40, 'Milk, eggs'),
        W('button', 'Save', 20, 110, 120),
        W('button', 'Delete note', 160, 110, 180),
        W('link', 'Recipe ideas (www.example.com)', 20, 180, 420, 20),
        W('field', 'Shop PIN', 20, 230, 220, 36),
        W('password', 'Password', 20, 290, 220, 36),
        // A check box: read_window lists it (phase 5), toggle_element ticks it.
        W('check', 'Word wrap', 20, 350, 160, 30),
      ],
    },
    {
      x: 1500,
      y: 60,
      w: 380,
      h: 520,
      exe: 'C:\\Program Files\\WindowsApps\\Microsoft.WindowsCalculator_11.2410.0.0_x64__8wekyb3d8bbwe\\CalculatorApp.exe',
      aumid: 'microsoft.windowscalculator_8wekyb3d8bbwe',
      class: 'ApplicationFrameWindow',
      title: 'Calculator',
      widgets: [W('text', 'Display', 20, 50, 340, 40, '0'), W('key', '7', 20, 120, 80, 60), W('key', '8', 110, 120, 80, 60), W('key', '+', 200, 120, 80, 60), W('key', '=', 290, 120, 70, 60), W('key', 'C', 20, 190, 80, 60)],
    },
    {
      x: 300,
      y: 700,
      w: 900,
      h: 300,
      exe: 'C:\\Program Files\\WindowsApps\\Microsoft.WindowsTerminal_1.21.3231.0_x64__8wekyb3d8bbwe\\WindowsTerminal.exe',
      aumid: 'microsoft.windowsterminal_8wekyb3d8bbwe',
      class: 'CASCADIA_HOSTING_WINDOW_CLASS',
      title: 'Terminal',
      widgets: [W('field', 'Prompt', 20, 50, 840, 34, 'PS C:\\Users\\Sam>')],
    },
    { x: 2000, y: 60, w: 1000, h: 600, exe: 'C:\\Program Files\\WindowsApps\\Microsoft.Paint_11.2410.0.0_x64__8wekyb3d8bbwe\\PaintApp\\mspaint.exe', aumid: 'microsoft.paint_8wekyb3d8bbwe', class: 'MSPaintApp', title: 'Paint' },
    { x: 0, y: 1040, w: 1920, h: 40, exe: 'C:\\Windows\\explorer.exe', class: 'Shell_TrayWnd' },
    { x: 0, y: 0, w: 1920, h: 1080, exe: 'C:\\Windows\\explorer.exe', class: 'Progman' },
  ],
  cursor: [600, 400],
};

/** `set --clock`'s time: "now" (or nothing) → null; "HH:MM" → today then; else any date `Date` reads. */
function clockAt(v) {
  const s = String(v ?? '').trim();
  if (!s || s === 'now' || s === 'true') return null;
  const hm = /^(\d{1,2}):(\d{2})$/.exec(s);
  if (hm) {
    const d = new Date();
    d.setHours(Number(hm[1]), Number(hm[2]), 0, 0);
    return d.getTime();
  }
  const ms = Date.parse(s);
  if (!Number.isFinite(ms)) throw new Error(`set --clock: "${s}" isn't a time ("23:40", "2026-11-18T23:40" or "now")`);
  return ms;
}

async function state() {
  readRecord();
  const conversations = ((await tryInvoke('chats_list', {}, [])) ?? []).map((c) => ({ id: c.id, title: c.title, updated: c.updatedTs, messages: c.count ?? c.messages ?? undefined }));
  const pages = (await tryInvoke('e2e_page_views', {}, [])) ?? [];
  const agents = (await agentsNow()).map((a) => ({ id: a.id, folder: a.folder, status: a.status, conversation: a.conversation, plan: a.plan, text: cut(a.text ?? '', 200), files: a.files, error: a.error ?? null }));
  const approvals = (await tryInvoke('claude_approve_pending', {}, [])) ?? [];
  const cards = turn && !turn.finalized ? pendingOf(cardsOf(turn.items, { approvals, agents: await agentsNow() })).filter((c) => !turn.answered.has(c.id)) : cardsOf([], { approvals, agents: await agentsNow() });
  const opened = (await tryInvoke('e2e_opened_apps', {}, [])) ?? [];
  // A Code chat's file links opened in an editor or the default app: the command each would have run.
  const filesOpened = (await tryInvoke('e2e_file_opens', {}, [])) ?? [];
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
    filesOpened,
  };
}

/** Yap Code's agents now: each one's state, the cards they wait on, and the reports they put in their chats. */
async function agentsView() {
  readRecord();
  const list = await agentsNow();
  const approvals = (await tryInvoke('claude_approve_pending', {}, [])) ?? [];
  const reports = [];
  // Every agent that ended has told its chat: its latest turn's report is in,
  // and answered (the chat's last message isn't a report still waiting for
  // its answer).
  let reportsIn = true;
  for (const conv of [...new Set(list.map((x) => x.conversation).filter((c) => c != null))]) {
    const c = await tryInvoke('chat_get', { id: conv });
    const messages = c?.messages ?? [];
    const ended = list.filter((x) => x.conversation === conv && ['done', 'failed'].includes(String(x.status).toLowerCase()));
    if (ended.some((x) => !(x.turnList?.length ? x.turnList.at(-1).reportMessageId : x.reportMessageId)) || (ended.length && messages.at(-1)?.kind === 'agentReport')) reportsIn = false;
    messages.forEach((m, i) => {
      if (m.kind !== 'agentReport') return;
      const reply = messages.slice(i + 1).find((n) => n.role === 'assistant' && n.kind !== 'agentReport');
      reports.push({ conversation: conv, report: cut(m.text, 1500), answer: reply ? cut(reply.text, 1500) : null });
    });
  }
  return {
    // Its latest turn's model and effort, and why Chat sized it so (agents/sizing.rs).
    agents: list.map((x) => ({ id: x.id, folder: x.folder, status: x.status, conversation: x.conversation, mode: x.mode, plan: x.plan, readOnly: !!x.turnList?.at(-1)?.readOnly, model: x.turnList?.at(-1)?.model || x.model || null, effort: x.turnList?.at(-1)?.effort || x.effort || null, sizing: x.turnList?.at(-1)?.sizing ?? null, text: cut(x.text ?? '', 600), files: x.files, commands: x.commands, denials: x.denials, error: x.error ?? null, cost: x.cost })),
    pending: cardsOf([], { approvals, agents: list }).filter((c) => c.state === 'asking'),
    reports,
    reportsIn,
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
  // The report a finished agent puts in its chat comes after: the chat's own
  // model words it, a few seconds on a real plan (11 Nov 2026: a script that
  // looked for the report card right away never found it, and its next
  // question got in ahead of the report). Up to 45 s more for it.
  await sleep(1000);
  let view = await agentsView();
  const reportBy = Date.now() + 45_000;
  while (!view.reportsIn && !view.pending.length && Date.now() < reportBy && !view.agents.some((x) => ['queued', 'running'].includes(String(x.status).toLowerCase()))) {
    await sleep(1000);
    view = await agentsView();
  }
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

// ---------------------------------------------------------------- the lab's own turns (16 Nov 2026)

/**
 * **A turn of the lab's own**: something done to the test Yap that isn't a question — an auto-archive run, a kept
 * page saved, a Code chat's summary read — with shots and a transcript as a question's, so a script's `expect`, the
 * sheet and `--chat same` go on from it (its conversation is the last turn's unless `fn` sets `t.conversation`).
 * `fn(t)` does it and returns what it found, laid into the turn's JSON; `windows`: its shots' windows, brought up by
 * `show(t)` first. A failure is the turn's (`status: "failed"`, `error`), never the command's.
 */
async function actionTurn(question, fn, a = {}, { windows = ['main'], show = null } = {}) {
  if (turn && !turn.finalized) throw new Error(`Turn ${turn.n} is still going (${turn.status}): wait for it, answer its card, or cancel it`);
  readRecord();
  const t = new Turn({ question, spoken: false, since: lastSeq(), askedAt: 0, ...shotsFor(a) });
  t.kind = 'lab';
  t.conversation = turn?.conversation ?? null;
  t.windows = windows;
  t.show = show;
  turn = t;
  let found = {};
  try {
    found = (await fn(t)) ?? {};
    t.status = 'answered';
  } catch (e) {
    t.status = 'failed';
    t.error = e.message.split('\n')[0];
    found = e.found ?? {};
  }
  t.endedAt = Date.now();
  readRecord();
  if (t.shotsOn) await shotSet(t, { set: 'end' }).catch((e) => t.shotErrors.push(`shots: ${e.message.split('\n')[0]}`));
  const result = { turn: t.n, kind: 'lab', status: t.status, conversation: t.conversation, question, ...found, error: t.error, ms: t.endedAt - t.startedAt };
  withShots(t, result);
  fs.mkdirSync(TURNS, { recursive: true });
  t.transcriptFile = path.join(TURNS, `${String(t.n).padStart(3, '0')}.json`);
  result.transcript = t.transcriptFile;
  t.transcriptData = { result, record: t.items };
  fs.writeFileSync(t.transcriptFile, JSON.stringify(t.transcriptData, null, 2));
  t.result = result;
  t.finalized = true;
  writeShotIndex(t);
  log(`turn ${t.n} (the lab's own) ${t.status}`);
  return result;
}

/** A failure carrying what was found so far (the turn's JSON keeps it). */
const failWith = (message, found) => Object.assign(new Error(message), { found });

/** Chat open on conversation `id` (the Chat view, its thread shown), as Yap's own "Open in Chat" opens one. */
async function showConversation(id) {
  await onScreen();
  await openChatView().catch(() => {});
  await main().evaluate((p) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', { event: 'yap-assistant-turn', payload: p }), { phase: 'open', conversationId: id, show: true });
  await waitFor(async () => (await main().locator(`.conv.active[data-conv="${id}"]`).count()) > 0, 10_000, `conversation ${id} to open`).catch(() => {});
}

/** The conversation an action is about: `--chat <id>`, `same` / none (the last turn's). */
function chatOf(a) {
  const c = a.chat ?? 'same';
  if (c === 'same') {
    if (!turn?.conversation) throw new Error('Which chat? --chat <id> (or ask something first: --chat same is the last turn’s)');
    return turn.conversation;
  }
  const id = Number(c);
  if (!Number.isFinite(id)) throw new Error(`--chat is a conversation's id or same, not "${c}"`);
  return id;
}

// ---- routines (chat/routines.rs) ----

/** A routine's run as the turn's JSON shows it. */
function runOf(r) {
  if (!r) return null;
  const at = (s) => (Number(s) > 0 ? new Date(Number(s) * 1000).toISOString() : null);
  return { id: r.id, routine: r.routine, command: r.command, cause: r.cause, state: r.state, why: r.why || null, chat: r.chat || null, note: r.note || null, thread: r.thread || null, about: r.about || null, queued: at(r.queued), started: at(r.started), ended: at(r.ended), slot: at(r.slot) };
}

/**
 * The Skill a routine runs: `--skill <command>` (Yap's own or the person's: `standup`, `/fun-fact`), made first as
 * one of the person's own when `--instructions` are given (`--name`, `--runs-on meeting|meetings|both`), as
 * Settings → Skills makes one. `{ id, command }`.
 */
async function skillFor(a) {
  const command = String(a.skill ?? '').replace(/^\//, '').trim();
  if (!command) throw new Error('routine new needs --skill <command> (with --instructions "<what it does>" to make one first)');
  const all = (await invoke('skills_list')) ?? [];
  let skill = all.find((s) => s.command === command) ?? null;
  if (a.instructions) {
    if (skill?.builtin) throw new Error(`/${command} is one of Yap's own Skills: name another --skill to make one`);
    const runsOn = a['runs-on'] ?? (a['with-note'] ? 'meeting' : 'both');
    skill = await invoke('skill_save', { skill: { id: skill?.id ?? '', name: a.name ?? command, command, description: '', instructions: String(a.instructions), runsOn } });
  }
  if (!skill) throw new Error(`No Skill /${command}: --instructions "<what it does>" makes one`);
  return { id: skill.id, command: skill.command };
}

/** `--at HH:MM`, `--in <minutes>`, `--after-meeting [--folder <f>] [--outside]` → a routine's trigger. */
function triggerOf(a) {
  if (a['after-meeting']) return { kind: 'meetingEnd', folder: a.folder ?? '', outside: !!a.outside };
  const time = a.at ? String(a.at) : minutesFrom(new Date(), a.in ?? 2);
  if (!/^\d{1,2}:\d{2}$/.test(time)) throw new Error(`--at is HH:MM (the PC's own time), not "${time}"`);
  return { kind: 'schedule', days: a.days ?? 'daily', weekday: Number(a.weekday ?? 0), time };
}

/**
 * Its next run, as a turn of the lab's own: started now, over when Settings' runs list says the run is (`tick`); a
 * routine's own chat shows its thread, one with the meeting's note shows Notes on it. Before its shots, the Yap bar
 * (its one quiet card, while it's there: Yap's window isn't in front in a lab run).
 */
/** The last routine `routine new` made (`routine now` with no id runs it). */
let lastRoutine = null;

async function routineTurn(routine, a, question) {
  // A routine's turn still waiting for its time (`routine new --at` hours away: no run yet) gives way to the next
  // `routine new` or `routine now` (16 Nov 2026: Run now on a routine made for later was refused until its time).
  if (turn && !turn.finalized && turn.routine && !turn.run) {
    turn.status = 'stopped';
    turn.error = 'Set aside before its time: the lab ran another routine turn';
    turn.endedAt = Date.now();
    await finalize(turn);
  }
  if (turn && !turn.finalized) throw new Error(`Turn ${turn.n} is still going (${turn.status}): wait for it first`);
  readRecord();
  const t = new Turn({ question, spoken: false, since: lastSeq(), askedAt: 0, ...shotsFor(a) });
  t.kind = 'routine';
  t.routine = { id: routine.id, after: Math.floor(Date.now() / 1000), chatBefore: Number(routine.chat) || 0 };
  lastRoutine = routine.id;
  // `--away`: the person in another app while it runs (test mode's focus), so its one quiet card goes on the Yap bar
  // (never while Yap's window is in front): the bar's shot waits for it, then the person is back in Yap.
  t.away = !!a.away;
  const card = () => yap.overlay.getByRole('status').filter({ hasText: /Routine/ }).first();
  t.preShots = t.away ? [{ set: 'notice', prefix: 'notice-', windows: ['bar'], before: () => card().waitFor({ state: 'visible', timeout: 10_000 }) }] : [];
  if (routine.to === 'note') {
    t.windows = ['main'];
    t.show = async (tt) => {
      if (tt.run?.note) await invoke('routine_open', { chat: null, note: tt.run.note }).catch(() => {});
      await sleep(1200);
    };
  }
  turn = t;
  return t;
}

/**
 * `routine new|now|list|on|off|delete` (chat/routines.rs, 16 Nov 2026): Skills that run by themselves, on the test
 * Yap's own clock and meetings.
 * - `new --skill <command> [--instructions "<…>" --name <n>] [--in <minutes> (2) | --at HH:MM | --after-meeting
 *   [--with-note] [--folder <f>] [--outside]] [--words "<…>"]`: saved as Settings' form saves one; its next run is
 *   a turn of the lab's own — `wait` (or `routine wait`) until it's over, its answer and shots as a question's;
 * - `now <id>`: Settings' Run now, waited on the same way (`--no-wait`);
 * - `list`: Settings' list (`routines_list`); `on|off|delete <id>`.
 */
async function routineCmd(a) {
  const [sub = 'list', id] = a._;
  if (sub === 'list') return { listing: await invoke('routines_list') };
  if (sub === 'on' || sub === 'off') {
    await invoke('routine_set_on', { id, on: sub === 'on' });
    return { routine: id, on: sub === 'on' };
  }
  if (sub === 'delete') {
    await invoke('routine_delete', { id });
    return { deleted: id };
  }
  if (sub === 'wait') {
    if (!turn?.routine) throw new Error('No routine is waited on: routine new or routine now first');
    return settle(turn, Number(a.timeout ?? 600));
  }
  if (sub === 'new') {
    const skill = await skillFor(a);
    const trigger = triggerOf(a);
    const to = a['with-note'] ? 'note' : 'chat';
    if (to === 'note' && trigger.kind !== 'meetingEnd') throw new Error('--with-note is for an --after-meeting routine');
    const saved = await invoke('routine_save', { routine: { id: '', skill: skill.id, words: String(a.words ?? ''), trigger, on: true, to } });
    const listed = ((await invoke('routines_list')).routines ?? []).find((r) => r.id === saved.id);
    const when = trigger.kind === 'schedule' ? `${trigger.days} at ${trigger.time}` : `after a meeting${trigger.folder ? ` in ${trigger.folder}` : ''}${to === 'note' ? ', with its note' : ''}`;
    const t = await routineTurn(saved, a, `Routine /${skill.command} (${when})`);
    if (t.away) await invoke('e2e_focus', { focused: false });
    return {
      routine: { id: saved.id, skill: skill.command, trigger, to, next: listed?.next ? new Date(listed.next * 1000).toISOString() : null },
      turn: t.n,
      status: 'running',
      hint: trigger.kind === 'schedule' ? `It runs at ${trigger.time} (the scheduler looks every 30 s): npm run lab -- wait` : 'It runs once a meeting ends and its summary is written: npm run lab -- meeting "<title>", then wait',
    };
  }
  if (sub === 'now') {
    const which = id ?? lastRoutine;
    if (!which) throw new Error('routine now <id> (routine list has them; none: the last one routine new made)');
    const r = ((await invoke('routines_list')).routines ?? []).find((x) => x.id === which);
    if (!r) throw new Error(`No routine ${which}`);
    const t = await routineTurn(r, a, `Routine /${r.command} (Run now)`);
    if (t.away) await invoke('e2e_focus', { focused: false });
    await invoke('routine_run_now', { id: which });
    return a['no-wait'] ? snapshot(t) : settle(t, Number(a.timeout ?? 600));
  }
  throw new Error('routine new|now|wait|list|on|off|delete');
}

/** A made-up meeting's lines (fictional people: support/fake-llm.js's own kind of words). */
const MEETING_LINES = [
  { source: 'them', text: 'Thanks everyone. The deposit landed this quarter, so the budget looks worse than it is for now.' },
  { source: 'you', text: 'Understood. Alice, can you send the revised budget by Friday so we can close this out?' },
  { source: 'them', text: 'Yes, I will send the revised budget by Friday afternoon.' },
  { source: 'you', text: 'Great. We agreed to keep the launch on the third of March.' },
];

/**
 * `meeting "<title>" [--folder <f>] [--lines "them: …|you: …"]` (16 Nov 2026): a made-up meeting recorded and ended
 * in the test Yap, as the routines spec does it — a note, the recorder started, test mode's lines fed in, ended —
 * so an "after a meeting" routine runs on it once its summary is written (or couldn't be). Fictional words only.
 */
async function meetingCmd(a) {
  const title = String(a._[0] ?? 'Quick sync').trim() || 'Quick sync';
  const lines = a.lines
    ? String(a.lines)
        .split('|')
        .map((l) => /^\s*(you|them)\s*:\s*(.+)$/i.exec(l))
        .filter(Boolean)
        .map((m) => ({ source: m[1].toLowerCase(), text: m[2].trim() }))
    : MEETING_LINES;
  if (!lines.length) throw new Error('--lines is "them: …|you: …"');
  const note = await invoke('note_create', { title, folder: a.folder ?? null, content: null, source: null });
  await invoke('meeting_start', { noteId: note.id });
  const recording = async () => (await invoke('meeting_state')).recording;
  await waitFor(recording, 20_000, 'the meeting to record');
  const ts = Math.floor(Date.now() / 1000);
  await invoke('e2e_meeting_feed', { segments: lines.map((l, i) => ({ ...l, ts: ts + i * 8 })) });
  await invoke('meeting_end', {});
  await waitFor(async () => !(await recording()), 30_000, 'the meeting to end');
  return { note: note.id, title, folder: a.folder ?? null, lines: lines.length };
}

// ---- auto-archive (chat/auto_archive.rs) ----

/**
 * `age <days> [--chat <id>|same|all]` (16 Nov 2026): chats made that many days older (`e2e_chat_age`: their start,
 * last use and messages' times moved back), so auto-archive can be tried on the lab's own chats without waiting.
 * `--chat all` (the default: every chat but a meeting's Ask thread or an incognito one).
 */
async function ageCmd(a) {
  const days = Number(a._[0] ?? a.days);
  if (!Number.isFinite(days) || days < 1) throw new Error('age <days> [--chat <id>|same|all]');
  const which = a.chat ?? 'all';
  const ids = which === 'all' ? null : [chatOf(a)];
  const aged = await invoke('e2e_chat_age', { ids, days });
  return { aged, days };
}

/**
 * `archive [--days 7|30|90] [--undo]` (16 Nov 2026): auto-archive's run now, as the daily one runs
 * (`e2e_chat_auto_archive`), its setting set first when `--days` is given ("Archive chats you haven't used in"), as a
 * turn of the lab's own: what it archived, what's left in Chat's list and in Archived, and the notice in Yap's window
 * with its Undo (`--undo` presses it, and says what came back). The main window's shots: the list and the toast.
 */
async function archiveCmd(a) {
  return actionTurn(
    `Auto-archive${a.days ? ` (${a.days} days)` : ''}${a.undo ? ', then Undo' : ''}`,
    async () => {
      const titles = async (cmd) => ((await invoke(cmd)) ?? []).map((c) => ({ id: c.id, title: c.title }));
      const before = new Set((await titles('chats_archived')).map((c) => c.id));
      // Chat's list on screen (the notice is Yap's window's), on the Chat side.
      await onScreen();
      await openChatView();
      await showList();
      if (a.days !== undefined) {
        const days = Number(a.days);
        if (![0, 7, 30, 90].includes(days)) throw new Error('--days is 7, 30 or 90 (0: never)');
        // (Setting it runs it at once, as the app does: on_config_saved.)
        await patchConfig({ chatArchiveDays: days });
      }
      const cfg = await invoke('get_config');
      const ran = await invoke('e2e_chat_auto_archive');
      const archivedList = await titles('chats_archived');
      const archived = archivedList.filter((c) => !before.has(c.id)).map((c) => c.id);
      const toast = main().getByRole('status').filter({ hasText: /Archived (a chat|\d+ chats) you hadn't used/ });
      const said = await toast
        .first()
        .waitFor({ state: 'visible', timeout: archived.length ? 8_000 : 1_500 })
        .then(async () => (await toast.first().innerText()).replace(/\s+/g, ' ').trim())
        .catch(() => null);
      const found = { days: cfg.chatArchiveDays ?? 0, archived, lastRun: ran, notice: said, list: await titles('chats_list'), archivedList };
      if (a.undo) {
        if (!said) throw failWith('No notice to Undo', found);
        await toast.first().getByRole('button', { name: 'Undo' }).click({ timeout: 5_000 });
        await sleep(800);
        found.undone = true;
        found.list = await titles('chats_list');
        found.archivedList = await titles('chats_archived');
      }
      return found;
    },
    a,
    { windows: ['main'] }
  );
}

// ---- a Code chat's summary: Changes through the lab's read-only gh (agents/prs.rs, agents/prs/lab.rs) ----

/**
 * `summary [--chat <id>|same]` (16 Nov 2026): a Code chat's summary beside it (the title bar's Summary), as a turn of
 * the lab's own: its Environment's branch, its Changes — the pull requests (`code_chat_pull_requests`, asked afresh)
 * and the rows it draws — and the panel's shots. With `set --gh on` (and the scratch workspace on a branch: `set
 * --git <branch>`) the rows are the branch's real pull requests on Yap's own repository.
 */
async function summaryCmd(a) {
  const id = chatOf(a);
  return actionTurn(
    `The summary of Code chat ${id}`,
    async (t) => {
      t.conversation = id;
      await showConversation(id);
      // Asked afresh first (gh's answer is kept 30 s: the panel, opened next, reads this one).
      const prs = await invoke('code_chat_pull_requests', { conversation: id, workspace: null, fresh: true });
      const panel = main().getByTestId('chat-summary');
      const button = main().getByTestId('chat-summary-button');
      // Open already: closed and opened again, so it reads what's true now.
      if (await panel.isVisible().catch(() => false)) {
        await button.click({ timeout: 10_000 });
        await panel.waitFor({ state: 'hidden', timeout: 5_000 }).catch(() => {});
      }
      if (!(await panel.isVisible().catch(() => false))) await button.click({ timeout: 10_000 });
      await panel.waitFor({ state: 'visible', timeout: 10_000 });
      // (The pointer off the title bar: the button's tooltip would sit over the panel in its shots.)
      await main().mouse.move(640, 500).catch(() => {});
      // Its Changes drawn: a row each, or the one quiet line.
      const changes = main().getByTestId('chat-summary-changes');
      await waitFor(async () => (await changes.locator('[data-testid="chat-summary-pr"], [data-testid="chat-summary-changes-note"]').count()) > 0, 30_000, 'the summary’s Changes').catch(() => {});
      const rows = await changes
        .locator('[data-testid="chat-summary-pr"]')
        .evaluateAll((els) => els.map((e) => ({ text: e.innerText.replace(/\s+/g, ' ').trim(), state: e.dataset.state, checks: e.dataset.checks, archive: !!e.querySelector('[data-testid="chat-summary-pr-archive"]') })))
        .catch(() => []);
      const note = await changes.getByTestId('chat-summary-changes-note').innerText({ timeout: 1_000 }).catch(() => null);
      const branch = await main().getByTestId('chat-summary-branch').innerText({ timeout: 1_000 }).catch(() => null);
      return { branch: branch?.replace(/\s+/g, ' ').trim() ?? null, prs: { status: prs.status, branch: prs.branch ?? null, list: prs.list }, rows, note };
    },
    a,
    { windows: ['panel'] }
  );
}

// ---- a kept page's Save as… and Open in browser (chat/made/pages.rs) ----

/**
 * **Windows' Save as, answered by the lab** (`kept save`): the dialog plugin's IPC (`plugin:dialog|save`, a fetch to
 * `ipc.localhost` in Yap's main window) answered with a file in the lab's own folder, and what it was asked
 * (`defaultPath`, `filters`) kept — no dialog opens, nothing of the person's is touched. Tauri's `invoke` can't be
 * replaced (it's a read-only property); its custom-protocol fetch can.
 */
async function answerSaveDialog(file) {
  await main().evaluate((answer) => {
    if (!window.__yapLabRealFetch) {
      const real = window.fetch.bind(window);
      window.__yapLabRealFetch = real;
      window.fetch = (input, init) => {
        const url = typeof input === 'string' ? input : (input?.url ?? '');
        let cmd = '';
        try {
          const u = new URL(url);
          if (u.hostname === 'ipc.localhost') cmd = decodeURIComponent(u.pathname.slice(1));
        } catch {
          /* not a URL */
        }
        const hook = window.__yapLabDialog;
        if (hook && cmd === 'plugin:dialog|save') {
          window.__yapLabDialog = null;
          let asked = null;
          try {
            asked = JSON.parse(typeof init?.body === 'string' ? init.body : '{}');
          } catch {
            /* a body of another kind */
          }
          window.__yapLabDialogAsked = asked;
          return Promise.resolve(new Response(JSON.stringify(hook.answer), { status: 200, headers: { 'Tauri-Response': 'ok', 'Content-Type': 'application/json' } }));
        }
        return real(input, init);
      };
    }
    window.__yapLabDialogAsked = undefined;
    window.__yapLabDialog = { answer };
  }, file);
}

/** The kept page's ⋯ beside the chat, and one of its items (`Save as…`, `Open in browser`), clicked. */
async function keptMenu(item) {
  const kept = main().getByTestId('chat-kept-page');
  if (!(await kept.isVisible().catch(() => false))) throw new Error('No kept page beside the chat: ask for one first (a page Yap keeps: save_page)');
  await kept.getByTestId('chat-kept-more').click({ timeout: 5_000 });
  await main().getByRole('menuitem', { name: item }).click({ timeout: 5_000 });
  return kept;
}

/** What a page file outside Yap holds: its size, title, the sandbox's policy, its libraries. */
function aboutPage(file) {
  const html = fs.readFileSync(file, 'utf8');
  return {
    bytes: Buffer.byteLength(html),
    title: /<title>([^<]*)<\/title>/i.exec(html)?.[1] ?? null,
    policy: /<meta[^>]+http-equiv=["']?Content-Security-Policy/i.test(html),
    scripts: (html.match(/<script\b[^>]*\bsrc=["'][^"']+/gi) ?? []).map((s) => s.replace(/^.*src=["']/i, '')),
    integrity: /<script\b[^>]*\bintegrity=/i.test(html),
    sendPrompt: /sendPrompt/.test(html),
  };
}

/**
 * The page copy opened as the person's browser would — in the lab's own headless browser instead, never the
 * person's: light and dark (`prefers-color-scheme`), its console errors and the policy's refusals kept, a shot of
 * each into the turn's folder (window "browser").
 */
async function renderOutside(t, file, themes) {
  const b = await labBrowser();
  const out = { errors: [], refused: [], shots: [] };
  try {
    for (const theme of themes.map((x) => x ?? 'light')) {
      const ctx = await b.newContext({ viewport: { width: 900, height: 700 }, colorScheme: theme });
      const page = await ctx.newPage();
      await page.addInitScript(() => document.addEventListener('securitypolicyviolation', (e) => (window.__refused = [...(window.__refused ?? []), `${e.violatedDirective} ${e.blockedURI}`])));
      page.on('console', (m) => m.type() === 'error' && out.errors.push(cut(m.text(), 200)));
      page.on('pageerror', (e) => out.errors.push(cut(e.message, 200)));
      await page.goto(pathToFileURL(file).href, { waitUntil: 'load', timeout: 30_000 });
      await page.waitForTimeout(1_200);
      out.refused.push(...((await page.evaluate(() => window.__refused ?? []).catch(() => [])) ?? []));
      const dir = turnShotDir(t);
      fs.mkdirSync(dir, { recursive: true });
      const shot = path.join(dir, `outside-${String(out.shots.length + 1).padStart(2, '0')}-browser-${theme}.png`);
      const png = await page.screenshot({ path: shot });
      const { width, height } = pngSize(png);
      out.shots.push(shot);
      t.shots.push({ file: shot, window: 'browser', theme, set: 'outside', width, height, dpr: 1 });
      await ctx.close();
    }
  } finally {
    await b.close().catch(() => {});
  }
  out.errors = [...new Set(out.errors)];
  out.refused = [...new Set(out.refused)];
  return out;
}

/**
 * `kept save [--name <file>]` / `kept open` (16 Nov 2026): the kept page beside the chat, outside Yap, through its ⋯
 * as the person does it — as a turn of the lab's own.
 * - **save**: Save as… with Windows' dialog answered by the lab (`answerSaveDialog`: a file in the lab's own
 *   `saved/` folder; what the dialog was asked kept), the toast, and the file read back;
 * - **open**: Open in browser — test mode keeps the copy's address instead of opening the person's browser
 *   (`e2e_opened_urls`) — then the copy read back and opened in the lab's own headless browser, light and dark
 *   (its shots in the sheet as "browser"; its console errors and the page policy's refusals listed).
 */
async function keptCmd(a) {
  const [how = 'save'] = a._;
  if (how !== 'save' && how !== 'open') throw new Error('kept save|open');
  return actionTurn(
    how === 'save' ? 'Save as… on the kept page' : 'Open in browser on the kept page',
    async (t) => {
      if (how === 'save') {
        const dir = path.join(LAB, 'saved');
        fs.mkdirSync(dir, { recursive: true });
        const name = String(a.name ?? `page-${Date.now()}`).replace(/[^\w .-]+/g, '-');
        const file = path.join(dir, name);
        await answerSaveDialog(file);
        await keptMenu('Save as…');
        const toast = main().getByRole('status').filter({ hasText: /Page saved|Couldn't save the page/ });
        const said = await toast
          .first()
          .waitFor({ state: 'visible', timeout: 10_000 })
          .then(async () => (await toast.first().innerText()).replace(/\s+/g, ' ').trim())
          .catch(() => null);
        const asked = await main().evaluate(() => window.__yapLabDialogAsked ?? null);
        if (asked === undefined || asked === null) throw failWith('The Save as dialog was never asked for (or not through the custom-protocol IPC)', { toast: said });
        const wrote = [file, `${file}.html`].find((f) => fs.existsSync(f)) ?? null;
        const found = { dialog: asked.options ?? asked, toast: said, file: wrote, ...(wrote ? { page: aboutPage(wrote) } : {}) };
        if (!wrote) throw failWith('Nothing was written', found);
        return found;
      }
      const before = ((await invoke('e2e_opened_urls')) ?? []).length;
      await keptMenu('Open in browser');
      let url = null;
      await waitFor(async () => {
        const all = (await invoke('e2e_opened_urls')) ?? [];
        url = all.slice(before).find((u) => u.startsWith('file:')) ?? null;
        return !!url;
      }, 10_000, 'the page’s copy to be opened').catch(() => {});
      if (!url) throw failWith('Open in browser opened nothing', {});
      const file = fileURLToPath(url);
      const found = { opened: url, file, page: fs.existsSync(file) ? aboutPage(file) : null };
      if (!found.page) throw failWith('The copy it opened isn’t there', found);
      if (t.shotsOn) found.outside = await renderOutside(t, file, t.themes);
      return found;
    },
    a,
    { windows: ['panel'] }
  );
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
    // **While a turn goes** (14 Nov 2026, the message box's queue): typed and sent in the same chat as the person
    // would mid-answer — no turn of the lab's own; the queue, a ✓✓ and the answers show in the thread (`probe`, `shot`).
    if (a.while) {
      const box = main().getByRole('textbox', { name: 'Message' });
      await box.fill(String(a.text ?? ''), { timeout: 10_000 });
      await main().getByRole('button', { name: 'Send', exact: true }).click({ timeout: 10_000 });
      return { sentWhile: true, turn: turn?.n ?? null, turnStatus: turn?.status ?? null };
    }
    // **A question card waiting, answered out loud** (chat/cards/ask.rs): asked out loud in the same chat, the
    // words are its answer and the turn it belongs to goes on, as `answer <id> said "<words>"`.
    if (a.spoken && (a.chat ?? 'new') === 'same' && turn && !turn.finalized) {
      const card = await waitingQuestion();
      if (card) return COMMANDS.answer({ ...a, _: [card.id, 'said', String(a.text ?? '')] });
    }
    // (Edit the last message to these words: in the chat shown.)
    const attach = Array.isArray(a.attach) ? a.attach : a.attach ? String(a.attach).split(',').filter(Boolean) : null;
    const how = a.edit ? 'edit' : 'ask';
    const text = a.text;
    const t = await startTurn({ text, spoken: !!a.spoken && how === 'ask', away: !!a.away && !!a.spoken && how === 'ask', chat: how === 'ask' ? (a.chat ?? 'new') : 'same', how, autoCards: ['allow', 'deny'].includes(a.cards) ? a.cards : null, pictures: how === 'ask' && Array.isArray(a.pictures) ? a.pictures : null, attach: how === 'ask' ? attach : null, style: typeof a.style === 'string' ? a.style : null, incognito: !!a.incognito, research: !!a.research && how === 'ask', project: typeof a.project === 'string' && how === 'ask' ? { name: a.project, instructions: typeof a['project-instructions'] === 'string' ? a['project-instructions'] : null, files: projectFiles(a['project-files']), page: !!a['project-page'] } : null, ...shotsFor(a) });
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
  click: async (a) => click(a),
  spoken: async (a) => spokenCmd(a),
  copied: async (a) => copiedCmd(a),
  // Computer use's guided tour (computer/teach.rs): the tooltip shown (a test Yap never puts it on the real screen), Next or Exit pressed on it.
  teach: async (a) => {
    const how = a._[0];
    if (how && !['next', 'exit'].includes(how)) throw new Error('teach [next|exit]: the guided tour’s tooltip, and Next or Exit pressed on it');
    const out = await invoke('e2e_computer_teach', how ? { press: how } : {});
    if (how && turn && !turn.finalized) turn.status = 'running';
    return out;
  },
  type: async (a) => typeKeys(a),
  side: async (a) => side(a),
  pick: async (a) => pick(a),
  set: async (a) => setThings(a),
  answer: async (a) => {
    const [id, how = 'allow', ...rest] = a._;
    if (!id) throw new Error('Which card? answer <card-id> allow|deny|always, or a question: pick "<option>"');
    if (!['allow', 'deny', 'always', 'pick', 'said', 'edit', 'revise'].includes(how)) throw new Error('A card is answered allow, deny or always (a question: pick "<option>", or said "<words>"; an outline: edit \'<json>\' or said; a plan: revise "<quote>::<comment>|…")');
    const pick = a.pick ?? rest.join(' ');
    if (how === 'pick' && !String(pick).trim()) throw new Error('pick what? answer <card-id> pick "<option>" ("A|B" for several questions, "A,B" for several picks)');
    if (how === 'said' && !String(pick).trim()) throw new Error('said what? answer <card-id> said "<words>" (a question asked out loud)');
    readRecord();
    const approvals = (await tryInvoke('claude_approve_pending', {}, [])) ?? [];
    const cards = cardsOf(turn?.items ?? [], { approvals, agents: await agentsNow() });
    // (A card the record hasn't shown: answered by its kind.)
    const prefix = /^(confirm|approve|plan):/.exec(id)?.[1];
    // (A question card the record hasn't shown can't be answered by its labels: it must be in the turn.)
    const card = cards.find((c) => c.id === id) ?? (prefix ? { id, kind: prefix === 'confirm' ? 'page' : prefix } : null);
    if (!card) throw new Error(`No card ${id}: the cards are ${cards.map((c) => c.id).join(', ') || 'none'}`);
    if (how === 'always' && card.kind !== 'connector') throw new Error('Only a connector’s card has Always allow');
    if (how === 'pick' && card.kind !== 'question') throw new Error('Only a question card is answered with pick');
    // (A yes-or-no card asked out loud — a page action's, an agent's, computer use's access card — takes a spoken yes or no too.)
    if (how === 'said' && card.kind !== 'question' && card.kind !== 'outline' && !turn?.spoken) throw new Error('Only a question card or an outline, or any card of a question asked out loud, is answered with said');
    if (how === 'edit' && card.kind !== 'outline') throw new Error('Only an outline is answered with edit');
    if (how === 'revise' && !(card.kind === 'approve' && card.tool === 'ExitPlanMode')) throw new Error("Only an agent's plan card (approve:<n>, ExitPlanMode) is sent back with revise");
    if (how === 'revise' && !planCommentsOf(pick).length) throw new Error('revise with what? answer approve:<n> revise "<words of the plan>::<your comment>|::<a comment on the whole plan>"');
    const answered = await answerCard(card, how, pick);
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
  // The lab's own turns (16 Nov 2026): routines, a made-up meeting, auto-archive, a Code chat's summary, a kept page outside Yap.
  routine: async (a) => routineCmd(a),
  meeting: async (a) => meetingCmd(a),
  age: async (a) => ageCmd(a),
  archive: async (a) => archiveCmd(a),
  summary: async (a) => summaryCmd(a),
  kept: async (a) => keptCmd(a),
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
  const env = { YAP_LAB: '1', YAP_LAB_RECORD: RECORD, YAP_LAB_WINDOW: START.size ?? '1280x860', YAP_E2E_APPS: START_MENU };
  // `--real-screen`: robot mode, and computer use on the real screen (shell/realtest.rs `lab_screen`).
  if (START.realScreen) Object.assign(env, { YAP_REALTEST: '1', YAP_LAB_REAL_SCREEN: '1' });
  fs.rmSync(START_MENU, { force: true });
  // The real web and the person's own Claude Code — but with `--fake`, the
  // stand-ins (`--fake --live-web`: the fake AI on the real web, for the
  // lab's own checks; a stand-in for Claude Code still wins: YAP_CLAUDE_EXE).
  if (!START.fake || START.liveWeb) env.YAP_LAB_LIVE = '1';
  // Stand-in editors for "Open file links in" (agents/editors.rs): a JSON file of `[{id, program}]`
  // the person points `YAP_LAB_EDITORS` at — never started; `state`'s `filesOpened` has each command.
  if (process.env.YAP_LAB_EDITORS) env.YAP_E2E_EDITORS = process.env.YAP_LAB_EDITORS;
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
  // The on-device model (`--model ondevice`): test mode's stand-in path for the sidecar, pointed at the server the
  // session started on this PC (YAP_LAB_LOCAL_LLM), with an empty model file of that name, as the e2e suite's.
  let prepare;
  if (model.kind === 'local') {
    env.YAP_E2E_LOCAL_LLM = localServer(process.env.YAP_LAB_LOCAL_LLM);
    const { installStandInModel } = await harness('local-llm.js');
    prepare = (dir) => installStandInModel(dir, model.model);
  }
  const config = labConfig({ model, effort: START.effort, web: START.web, yapCode: START.yapCode, connectors: START.connectors, key: process.env.YAP_LAB_ANTHROPIC_KEY ?? '', fakeBase });
  fs.rmSync(RECORD, { force: true });
  const { launchYap } = await harness('yap.js');
  log('launching the test Yap', START.exe);
  yap = await launchYap({ name: 'lab', runsDir: RUNS, exe: START.exe, config, env, prepare });
  yap.child.on('exit', (code) => {
    if (!stopping) {
      log(`the test Yap exited (${code})`);
      shutdown(`the test Yap exited (${code})`, 'exited');
    }
  });
  await tryInvoke('e2e_main_on_screen', {});
  // Copy in the test Yap never reaches this PC's clipboard (`copied` reads what it copied).
  await holdClipboard();
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
