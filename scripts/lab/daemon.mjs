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
import { APP, labConfig, parseModel } from './options.mjs';
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
  constructor({ question, spoken, since, askedAt, autoCards = null, report = null }) {
    this.n = ++turns;
    this.question = question;
    this.spoken = spoken;
    this.since = since;
    this.askedAt = askedAt;
    this.autoCards = autoCards;
    this.report = report;
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
  return summarize({ turn: t, items: t.items, conv, pages, approvals: t.approvals, agents: await agentsNow() });
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
  if (t.report) result.report = writeReport(t, result, conv);
  fs.writeFileSync(
    transcript,
    JSON.stringify({ result, tools: toolsOf(t.items, { full: true }), conversation: conv, pages, agents, record: t.items }, null, 2)
  );
  t.result = result;
  t.finalized = true;
  log(`turn ${t.n} ${t.status} in ${result.ms} ms`);
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

async function startTurn({ text, spoken = false, chat = 'new', autoCards = null, report = null }) {
  if (!text?.trim()) throw new Error('Nothing to ask');
  if (turn && !turn.finalized) throw new Error(`Turn ${turn.n} is still going (${turn.status}): wait for it, answer its card, or cancel it`);
  readRecord();
  const since = lastSeq();
  const { askedAt } = await openChat(chat).catch((e) => {
    throw new Error(`Couldn't open Chat on ${chat || 'a new chat'}: ${e.message.split('\n')[0]}`);
  });
  const t = new Turn({ question: text, spoken, since, askedAt, autoCards, report });
  turn = t;
  try {
    if (spoken) {
      await invoke('e2e_assistant_ask', { text, listenMs: 300 });
    } else {
      const box = main().getByRole('textbox', { name: 'Message' });
      await box.fill(text, { timeout: 10_000 });
      await main().getByRole('button', { name: 'Send' }).click({ timeout: 10_000 });
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

function writeReport(t, result, conv) {
  fs.mkdirSync(REPORTS, { recursive: true });
  const stamp = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19);
  const file = path.join(REPORTS, `${t.report.name}-${stamp}.md`);
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

async function shot({ window: which = 'main' }) {
  fs.mkdirSync(SHOTS, { recursive: true });
  const stamp = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19);
  const file = path.join(SHOTS, `${stamp}-${which}.png`);
  if (which === 'main') await main().screenshot({ path: file });
  else if (which === 'bar') await yap.overlay.screenshot({ path: file });
  else if (which === 'panel') {
    const panel = main().getByTestId('chat-panel-slot');
    if (!(await panel.isVisible().catch(() => false))) throw new Error('Chat has no panel showing now');
    await panel.screenshot({ path: file });
  } else if (which === 'page') {
    // The pages beside the chat are WebView2s of their own (not Yap's): on
    // the same DevTools port, in a browser context of their own.
    const ours = new Set(Object.values(yap.pages));
    const views = (await tryInvoke('e2e_page_views', {}, [])) ?? [];
    const shown = views.find((v) => v.onScreen ?? v.on_screen) ?? views.at(-1);
    const pages = yap.browser.contexts().flatMap((c) => c.pages()).filter((p) => !ours.has(p) && !p.url().startsWith('http://tauri.localhost/'));
    const page = pages.find((p) => shown && p.url().replace(/\/+$/, '') === String(shown.url ?? shown.address ?? '').replace(/\/+$/, '')) ?? pages.at(-1);
    if (!page) throw new Error('No page beside the chat to take a shot of (or it isn\'t reachable over DevTools: use --window panel)');
    await page.screenshot({ path: file });
  } else throw new Error('--window is main, bar, panel or page');
  return { shot: file, window: which };
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
    lab: { model: START.model, effort: START.effort ?? null, web: START.web, yapCode: START.yapCode, connectors: START.connectors, fake: !!START.fake, dataDir: yap.dataDir, pid: yap.child.pid, upMs: Date.now() - control.startedAt, workspace },
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
    agents: list.map((x) => ({ id: x.id, folder: x.folder, status: x.status, conversation: x.conversation, mode: x.mode, plan: x.plan, text: cut(x.text ?? '', 600), files: x.files, commands: x.commands, denials: x.denials, error: x.error ?? null, cost: x.cost })),
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
    const t = await startTurn({ text: a.text, spoken: !!a.spoken, chat: a.chat ?? 'new', autoCards: ['allow', 'deny'].includes(a.cards) ? a.cards : null });
    return a['no-wait'] ? snapshot(t) : settle(t, Number(a.timeout ?? 600));
  },
  wait: async (a) => {
    if (a.agents) return waitAgents(Number(a.timeout ?? 600));
    if (!turn) throw new Error('No turn yet: ask something first');
    return settle(turn, Number(a.timeout ?? 600));
  },
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
    const t = await startTurn({ text, chat: a.chat ?? 'new', autoCards: cards, report: { name, file } });
    const out = a['no-wait'] ? await snapshot(t) : await settle(t, Number(a.timeout ?? 540));
    if (!t.finalized) out.hint = 'The report is still being written; the lab saves it when the turn ends: npm run lab -- wait';
    return out;
  },
  fake: async (a) => {
    if (!fakeLlm) throw new Error('Only with start --fake (and a model that isn’t the plan)');
    if (a.script) fakeLlm.scriptChat(JSON.parse(a.script));
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
  const env = { YAP_LAB: '1', YAP_LAB_RECORD: RECORD };
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
  const out = { model, dataDir: yap.dataDir, windows: Object.keys(yap.pages), yapPid: yap.child.pid };
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
