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
import { conversationOf, endOf, cardsOf, pendingOf, summarize, toolsOf, cut, planCommentsOf } from './turns.mjs';
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

/** How long a Code chat's session's answer may take to land once its turn is over (its report, chat/turn/report.rs). */
const SESSION_GRACE_MS = 20_000;

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

async function startTurn({ text, spoken = false, chat = 'new', autoCards = null, report = null, shots = true, themes, pictures = null, attach = null, how = 'ask', style = null, incognito = false, project = null }) {
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
    // A new chat started from a project's page (chat/projects.rs), the project made first if need be.
    if (project) await newChatInProject(project);
    // The chat's style from Style under the box (chat/styles.rs), as the person picks it.
    if (style) await pickStyle(style);
    // A new chat made incognito with its ghost (chat/incognito.rs).
    if (incognito) {
      const ghost = main().getByTestId('chat-incognito');
      if ((await ghost.getAttribute('aria-pressed', { timeout: 10_000 })) !== 'true') await ghost.click({ timeout: 10_000 });
    }
    if (how === 'retry' || how === 'edit') {
      await branchTurn(how, text);
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
 * **Retry** the chat's last answer, or **Edit** the person's last message to `text` (15 Nov 2026, the Claude app's
 * message actions: chat/chats/branches.rs), as the person does — by its buttons: ↻ under the answer; the pencil,
 * the words in place, Save and send. A new version of the conversation from there (‹ 2/2 ›).
 */
async function branchTurn(how, text) {
  const col = main().locator('[data-testid="chat-column"]');
  if (how === 'retry') {
    await col.locator('.cbubble.assistant').last().hover({ timeout: 10_000 });
    await col.getByTestId('chat-retry').last().click({ timeout: 10_000 });
    await confirmIfAsked();
    return;
  }
  await col.locator('.cbubble.user').last().hover({ timeout: 10_000 });
  await col.getByTestId('chat-edit-message').last().click({ timeout: 10_000 });
  await col.getByTestId('chat-edit-input').fill(text, { timeout: 10_000 });
  await col.getByTestId('chat-edit-save').click({ timeout: 10_000 });
  await confirmIfAsked();
}

/**
 * A Code chat asks first: its Retry and Edit go through Rewind ("Retry from this message?", "Send the edited
 * message?", ChatThread.svelte). Its confirm pressed, as the person would (Lab, 15 Nov 2026: `--retry` waited on the
 * question for good). A plain chat asks nothing: back at once.
 */
async function confirmIfAsked() {
  const confirm = main().locator('[role="alertdialog"] .btns button:not([data-cancel])').first();
  try {
    await confirm.waitFor({ state: 'visible', timeout: 2_500 });
  } catch {
    return;
  }
  await confirm.click({ timeout: 5_000 });
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
  const m = main();
  await m.getByRole('button', { name: 'Projects', exact: true }).click({ timeout: 10_000 });
  await m.getByTestId('chat-projects').getByRole('button', { name }).first().click({ timeout: 10_000 });
  if (project.page) {
    await m.getByTestId('chat-project').getByRole('button', { name: `New chat in ${name}` }).waitFor({ state: 'visible', timeout: 10_000 });
    return;
  }
  await m.getByRole('button', { name: `New chat in ${name}` }).click({ timeout: 10_000 });
}

/** Style under the message box (chat/styles.rs): the one named `name` (any case), as the person picks it. */
async function pickStyle(name) {
  const m = main();
  await m.getByTestId('chat-style-chip').click({ timeout: 10_000 });
  const wanted = String(name).trim().toLowerCase();
  const items = m.getByRole('menu').first().getByRole('menuitemradio');
  // (The chip reads the styles before its menu opens: wait for it.)
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
      await box.getByRole('button', { name: 'Go', exact: true }).click({ timeout: 5_000 });
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
  } else if (kind === 'confirm' && (card.kind === 'question' || card.kind === 'outline') && how === 'said') {
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

/**
 * **Clicks a button of the test Yap's own** (`--css`, in `--window` main / thread / panel), as the person would
 * (14 Nov 2026, the message box's queue: a queued message's Send now and Cancel and edit). Only Yap's own windows,
 * never a page beside the chat (that's Claude's to act on, with its rules); the first match that shows.
 */
async function click({ window: which = 'main', css }) {
  if (!css) throw new Error('click needs --css');
  if (!['main', 'thread', 'panel'].includes(which)) throw new Error("click works in Yap's own window: main, thread or panel");
  const scope = which === 'panel' ? '[data-testid="chat-panel-slot"] ' : which === 'thread' ? '[data-testid="chat-column"] ' : '';
  const target = main().locator(`${scope}${css}`).first();
  await target.click({ timeout: 5_000 });
  return { window: which, css, clicked: true };
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
    // (Retry the last answer, or Edit the last message to these words: in the chat shown.)
    const attach = Array.isArray(a.attach) ? a.attach : a.attach ? String(a.attach).split(',').filter(Boolean) : null;
    const how = a.retry ? 'retry' : a.edit ? 'edit' : 'ask';
    const text = how === 'retry' ? String(a.text ?? '').trim() || 'Retry' : a.text;
    const t = await startTurn({ text, spoken: !!a.spoken && how === 'ask', chat: how === 'ask' ? (a.chat ?? 'new') : 'same', how, autoCards: ['allow', 'deny'].includes(a.cards) ? a.cards : null, pictures: how === 'ask' && Array.isArray(a.pictures) ? a.pictures : null, attach: how === 'ask' ? attach : null, style: typeof a.style === 'string' ? a.style : null, incognito: !!a.incognito, project: typeof a.project === 'string' && how === 'ask' ? { name: a.project, instructions: typeof a['project-instructions'] === 'string' ? a['project-instructions'] : null, page: !!a['project-page'] } : null, ...shotsFor(a) });
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
    if (how === 'said' && card.kind !== 'question' && card.kind !== 'outline') throw new Error('Only a question card or an outline is answered with said');
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
