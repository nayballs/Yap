// Yap Lab's feature scripts (docs/lab.md, `run`): a list of steps — ask,
// answer a card, wait, shot, set, expect — run against the running lab,
// each `expect` checked against the last turn (its tools with their whole
// arguments and results, the cards, the answer, its pictures and files),
// the agents, or what's on screen (over CDP). The checks are pure
// (`checkTurn`); the runner sends the lab's own commands.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { drawPng } from './png.mjs';

export const SCRIPTS = path.join(path.dirname(fileURLToPath(import.meta.url)), 'scripts');

/** The kinds of step, by their key. */
export const KINDS = ['ask', 'answer', 'wait', 'shot', 'set', 'expect', 'fake', 'note'];

/** Which kind a step is: its first key that's a kind. Pure. */
export function kindOf(step) {
  const k = KINDS.find((kind) => step && Object.prototype.hasOwnProperty.call(step, kind));
  if (!k) throw new Error(`A step is one of ${KINDS.join(', ')}: ${JSON.stringify(step).slice(0, 120)}`);
  return k;
}

/** A script's file: a path, or a name in scripts/lab/scripts/ (with or without .json / .mjs). */
export function scriptFile(name) {
  if (!name) throw new Error('Which script? run <script.json|.mjs> (scripts/lab/scripts/ has some)');
  const tries = [path.resolve(name), ...['', '.json', '.mjs'].map((ext) => path.join(SCRIPTS, `${name}${ext}`))];
  const found = tries.find((f) => fs.existsSync(f) && fs.statSync(f).isFile());
  if (!found) {
    const have = fs.existsSync(SCRIPTS) ? fs.readdirSync(SCRIPTS).filter((f) => /\.(json|mjs)$/.test(f)) : [];
    throw new Error(`No script "${name}"${have.length ? `: scripts/lab/scripts/ has ${have.join(', ')}` : ''}`);
  }
  return found;
}

/**
 * A script: `{ name, about?, needs?, steps }` from JSON, or a `.mjs` whose
 * default export is one (or a function `(helpers) => one`, helpers: `{
 * drawPng }`). Its steps checked for their kind.
 */
export async function loadScript(file) {
  let script;
  if (/\.json$/i.test(file)) script = JSON.parse(fs.readFileSync(file, 'utf8'));
  else {
    const mod = await import(pathToFileURL(file).href);
    script = typeof mod.default === 'function' ? await mod.default({ drawPng }) : mod.default;
  }
  if (!script || !Array.isArray(script.steps) || !script.steps.length) throw new Error(`${file}: a script has a list of steps`);
  script.steps.forEach(kindOf);
  return { name: script.name ?? path.basename(file).replace(/\.(json|mjs)$/i, ''), ...script, file };
}

// ---------------------------------------------------------------- matching

/** "/re/flags" → a RegExp; anything else null. Pure. */
function regexOf(p) {
  const m = typeof p === 'string' ? /^\/(.*)\/([a-z]*)$/s.exec(p) : null;
  return m ? new RegExp(m[1], m[2]) : null;
}
/** Text matches a pattern: "/re/" as a regex, else words found in it (any case). Pure. */
export function textMatches(pattern, text) {
  const re = regexOf(pattern);
  const s = String(text ?? '');
  return re ? re.test(s) : s.toLowerCase().includes(String(pattern).toLowerCase());
}
/** A name matches: "/re/" as a regex, else exactly. Pure. */
const nameMatches = (pattern, name) => (regexOf(pattern) ? regexOf(pattern).test(String(name ?? '')) : String(name ?? '') === String(pattern));
/** A value matches: "*" (there at all), "/re/", else equal. Pure. */
function valueMatches(want, got) {
  if (want === '*') return got !== undefined && got !== null && got !== '';
  if (regexOf(want)) return regexOf(want).test(typeof got === 'string' ? got : JSON.stringify(got));
  return JSON.stringify(got) === JSON.stringify(want) || String(got) === String(want);
}
/** A count matches: a number exactly, or `{ min, max }`. Pure. */
function countMatches(want, n) {
  if (typeof want === 'number') return n === want;
  return (want.min === undefined || n >= want.min) && (want.max === undefined || n <= want.max);
}
const countText = (want) => (typeof want === 'number' ? `${want}` : [want.min !== undefined ? `≥ ${want.min}` : '', want.max !== undefined ? `≤ ${want.max}` : ''].filter(Boolean).join(' and '));
const list = (v) => (v === undefined ? [] : Array.isArray(v) ? v : [v]);

/** A tool call's arguments as an object (a summed-up call's are a JSON string). Pure. */
function argsOf(call) {
  if (call.args && typeof call.args === 'object') return call.args;
  try {
    return JSON.parse(call.args);
  } catch {
    return {};
  }
}
/** A tool call's result as text: Yap's result object as JSON, Claude Code's content's text blocks. Pure. */
export function resultText(call) {
  const r = call.result;
  if (r === null || r === undefined) return '';
  if (typeof r === 'string') return r;
  if (Array.isArray(r)) return r.map((b) => (b?.type === 'text' ? b.text : b?.type === 'image' ? '[picture]' : JSON.stringify(b))).join('\n');
  return JSON.stringify(r);
}
/** A tool call's result as an object (Yap's tools answer JSON, over MCP as their first text block), or null. Pure. */
export function resultObject(call) {
  const r = call.result;
  if (r && typeof r === 'object' && !Array.isArray(r)) return r;
  const text = Array.isArray(r) ? r.find((b) => b?.type === 'text')?.text : typeof r === 'string' ? r : null;
  if (!text) return null;
  try {
    const v = JSON.parse(text);
    return v && typeof v === 'object' ? v : null;
  } catch {
    return null;
  }
}
/** `obj` at `path` ("data.scroll.y", "items.0.name"). Pure. */
export function at(obj, p) {
  return String(p).split('.').reduce((v, k) => (v === null || v === undefined ? undefined : v[k]), obj);
}

/** Does `call` match the tool check `want`? Each field it names must hold. Pure. */
export function callMatches(want, call) {
  if (want.name !== undefined && !nameMatches(want.name, call.name)) return false;
  if (want.via !== undefined && call.via !== want.via) return false;
  if (want.ok !== undefined && (call.ok !== false) !== want.ok) return false;
  if (want.refused !== undefined && !!call.refused !== want.refused) return false;
  const args = argsOf(call);
  if (want.args && !Object.entries(want.args).every(([k, v]) => valueMatches(v, args[k]))) return false;
  if (want.argsHas && !list(want.argsHas).every((k) => args[k] !== undefined && args[k] !== null)) return false;
  if (want.argsLack && list(want.argsLack).some((k) => args[k] !== undefined && args[k] !== null)) return false;
  const text = resultText(call);
  if (want.result !== undefined && !list(want.result).every((p) => textMatches(p, text))) return false;
  if (want.resultNot !== undefined && list(want.resultNot).some((p) => textMatches(p, text))) return false;
  if (want.path !== undefined) {
    const v = at(resultObject(call), want.path);
    if (want.equals !== undefined && !valueMatches(want.equals, v)) return false;
    if (want.gt !== undefined && !(Number(v) > want.gt)) return false;
    if (want.gte !== undefined && !(Number(v) >= want.gte)) return false;
    if (want.lt !== undefined && !(Number(v) < want.lt)) return false;
    if (want.matches !== undefined && !textMatches(want.matches, v ?? '')) return false;
    if (want.notMatches !== undefined && textMatches(want.notMatches, v ?? '')) return false;
    if ([want.equals, want.gt, want.gte, want.lt, want.matches, want.notMatches].every((x) => x === undefined) && v === undefined) return false;
  }
  return true;
}

/** A tool check in words. Pure. */
function toolWords(want) {
  const parts = [want.name ?? 'a tool'];
  if (want.args) parts.push(`args ${JSON.stringify(want.args)}`);
  if (want.argsHas) parts.push(`with ${list(want.argsHas).join(', ')}`);
  if (want.ok !== undefined) parts.push(want.ok ? 'ok' : 'not ok');
  if (want.refused !== undefined) parts.push(want.refused ? 'refused' : 'not refused');
  if (want.result !== undefined) parts.push(`result ~ ${list(want.result).join(' & ')}`);
  if (want.resultNot !== undefined) parts.push(`result !~ ${list(want.resultNot).join(' | ')}`);
  if (want.path !== undefined) {
    const how = ['equals', 'gt', 'gte', 'lt', 'matches', 'notMatches'].filter((k) => want[k] !== undefined).map((k) => `${k} ${JSON.stringify(want[k])}`);
    parts.push(`${want.path} ${how.join(' ') || 'there'}`);
  }
  return parts.join(', ');
}
/** The calls of `name` (or all), short, for a failure's detail. Pure. */
function callsDetail(calls, name) {
  const mine = name ? calls.filter((c) => nameMatches(name, c.name)) : calls;
  if (!mine.length) return `no call of ${name ?? 'any tool'} (called: ${[...new Set(calls.map((c) => c.name))].join(', ') || 'none'})`;
  return mine
    .slice(0, 6)
    .map((c) => `${c.name}(${JSON.stringify(argsOf(c)).slice(0, 120)}) → ${c.ok === false ? (c.refused ? 'refused: ' : 'failed: ') : ''}${resultText(c).replace(/\s+/g, ' ').slice(0, 200)}`)
    .join(' | ');
}

/**
 * **An `expect` checked**: `result` — the last turn (`ask`'s JSON), `calls`
 * — its tool calls (whole, from its transcript when there's one), `agents` —
 * the last `wait --agents` (or `agents`). Every check: `{ check, pass,
 * detail? }`. The screen's checks (`selector`, `visible`, `notVisible`) are
 * the runner's. Pure.
 */
export function checkTurn(expect, { result: turnResult, calls = turnResult?.tools ?? [], agents = null }) {
  const out = [];
  const result = turnResult ?? {};
  const add = (check, pass, detail) => out.push({ check, pass: !!pass, ...(pass ? {} : { detail }) });
  const needTurn = ['status', 'used', 'notUsed', 'refused', 'failed', 'card', 'answerContains', 'answerMatches', 'answerNotContains', 'pictures', 'files', 'documents', 'pagesShown', 'sources', 'tools', 'tool'];
  if (!turnResult && needTurn.some((k) => expect[k] !== undefined)) {
    add('a turn to check', false, 'no turn yet: ask first');
    return out;
  }
  if (expect.status !== undefined) add(`status ${list(expect.status).join(' or ')}`, list(expect.status).includes(result.status), `it was ${result.status}${result.error ? `: ${result.error}` : ''}`);
  for (const name of list(expect.used)) add(`used ${name}`, calls.some((c) => nameMatches(name, c.name)), callsDetail(calls, name));
  for (const name of list(expect.notUsed)) add(`didn't use ${name}`, !calls.some((c) => nameMatches(name, c.name)), callsDetail(calls, name));
  if (expect.refused !== undefined) {
    const refused = calls.filter((c) => c.refused);
    if (typeof expect.refused === 'string') add(`${expect.refused} refused`, refused.some((c) => nameMatches(expect.refused, c.name)), callsDetail(calls, expect.refused));
    else add(expect.refused ? 'something refused' : 'nothing refused', expect.refused ? refused.length > 0 : refused.length === 0, refused.length ? callsDetail(refused) : 'no call was refused');
  }
  if (expect.failed !== undefined) {
    const bad = calls.filter((c) => c.ok === false);
    add(expect.failed ? 'a call failed' : 'no call failed', expect.failed ? bad.length > 0 : bad.length === 0, bad.length ? callsDetail(bad) : 'every call was ok');
  }
  if (expect.card !== undefined) {
    const cards = result.cards ?? [];
    if (typeof expect.card === 'boolean') add(expect.card ? 'a card was asked' : 'no card was asked', expect.card ? cards.length > 0 : cards.length === 0, cards.length ? cards.map((c) => `${c.id} ${c.label}`).join(', ') : 'no card');
    else {
      const want = expect.card;
      const hit = cards.some((c) => Object.entries(want).every(([k, v]) => valueMatches(v, c[k])));
      add(`a card ${JSON.stringify(want)}`, hit, cards.length ? cards.map((c) => JSON.stringify({ id: c.id, kind: c.kind, act: c.act, label: c.label, does: c.does, state: c.state })).join(' | ') : 'no card');
    }
  }
  const answer = result.answer ?? '';
  for (const p of list(expect.answerContains)) add(`answer has ${p}`, textMatches(p, answer), `answer: ${answer.replace(/\s+/g, ' ').slice(0, 300)}`);
  for (const p of list(expect.answerMatches)) add(`answer ~ ${p}`, (regexOf(p) ?? new RegExp(p, 'i')).test(answer), `answer: ${answer.replace(/\s+/g, ' ').slice(0, 300)}`);
  for (const p of list(expect.answerNotContains)) add(`answer hasn't ${p}`, !textMatches(p, answer), `answer: ${answer.replace(/\s+/g, ' ').slice(0, 300)}`);
  const counts = { pictures: result.pictures, files: result.files, documents: result.documents, pagesShown: result.pagesShown, sources: result.sources, tools: calls };
  for (const [k, items] of Object.entries(counts)) {
    if (expect[k] === undefined) continue;
    const n = (items ?? []).length;
    add(`${k} ${countText(expect[k])}`, countMatches(expect[k], n), `${n}${n ? `: ${JSON.stringify(items).slice(0, 300)}` : ''}`);
  }
  for (const want of list(expect.tool)) add(`a call: ${toolWords(want)}`, calls.some((c) => callMatches(want, c)), callsDetail(calls, want.name));
  for (const want of list(expect.agent)) {
    const all = agents?.agents ?? [];
    const hit = all.filter((a) =>
      (want.status === undefined || list(want.status).map((s) => s.toLowerCase()).includes(String(a.status).toLowerCase())) &&
      (want.files === undefined || countMatches(want.files, (a.files ?? []).length)) &&
      (want.error === undefined || (want.error === null ? !a.error : textMatches(want.error, a.error ?? ''))) &&
      (want.folder === undefined || a.folder === want.folder) &&
      (want.mode === undefined || list(want.mode).includes(a.mode)) &&
      (want.plan === undefined || !!a.plan === !!want.plan) &&
      (want.readOnly === undefined || !!a.readOnly === !!want.readOnly)
    );
    const words = `an agent ${['status', 'folder', 'mode', 'plan', 'readOnly'].filter((k) => want[k] !== undefined).map((k) => `${k} ${list(want[k]).join('|')}`).concat(want.files !== undefined ? [`files ${countText(want.files)}`] : []).join(', ')}`;
    add(words.trim(), countMatches(want.count ?? { min: 1 }, hit.length), all.length ? all.map((a) => `${a.id}: ${a.status}, ${a.mode ?? '?'}${a.readOnly ? ' read only' : ''}${a.plan ? ', a plan' : ''}, ${(a.files ?? []).length} files${a.error ? `, ${a.error}` : ''}`).join(' | ') : 'no agents');
  }
  return out;
}

/** The screen's checks of an `expect`, as `probe` calls: `[{ check, args, want }]`. Pure. */
export function screenChecks(expect) {
  const out = [];
  for (const s of list(expect.selector)) {
    const window = s.window ?? 'main';
    const want = s.count !== undefined ? { count: s.count } : { present: s.present ?? true };
    out.push({ check: `${window}: ${s.css} ${want.count !== undefined ? `× ${countText(want.count)}` : want.present ? 'shows' : "doesn't show"}`, args: { window, css: s.css }, want });
  }
  for (const text of list(expect.visible)) out.push({ check: `the thread shows ${text}`, args: { window: 'thread', text }, want: { has: true } });
  for (const text of list(expect.notVisible)) out.push({ check: `the thread doesn't show ${text}`, args: { window: 'thread', text }, want: { has: false } });
  return out;
}

/** A probe's answer against what a screen check wants. Pure. */
export function probePasses(want, got) {
  if (want.has !== undefined) return !!got.has === want.has;
  if (want.count !== undefined) return countMatches(want.count, got.visible ?? 0);
  return want.present ? (got.visible ?? 0) > 0 : (got.visible ?? 0) === 0;
}

// ---------------------------------------------------------------- the runner

/** The steps that the rest depend on: when one fails, the script stops. */
const STOPS = new Set(['ask', 'answer', 'wait', 'set', 'fake']);

/** A turn's tool calls whole (its transcript), else as `ask` summed them up. */
function callsOf(result) {
  try {
    if (result?.transcript && fs.existsSync(result.transcript)) return JSON.parse(fs.readFileSync(result.transcript, 'utf8')).tools ?? result.tools ?? [];
  } catch {
    /* being written: the summed-up ones */
  }
  return result?.tools ?? [];
}

/** The card a step answers: an id, "next" (the first waiting), or `{ kind, label, … }`. */
function cardFor(want, cards) {
  if (typeof want === 'string' && want !== 'next') return want;
  const waiting = cards.filter((c) => c.state === 'asking');
  const hit = want === 'next' || want === true ? waiting[0] : waiting.find((c) => Object.entries(want).every(([k, v]) => valueMatches(v, c[k])));
  if (!hit) throw new Error(`No card waiting${typeof want === 'object' ? ` like ${JSON.stringify(want)}` : ''}: ${cards.map((c) => `${c.id} (${c.state})`).join(', ') || 'none'}`);
  return hit.id;
}

/**
 * **Run `script`** on the running lab: `send(cmd, args)` is the CLI's way
 * to it; `state` its `state` (its options: is it --fake, is Yap Code on).
 * `options`: `{ shots, theme }` from `run`'s own flags. `{ ok, steps,
 * failures, warnings, turns }`.
 */
export async function runScript(script, { send, state, options = {} }) {
  const out = { name: script.name, steps: [], failures: [], warnings: [], turns: [] };
  const lab = state.lab ?? {};
  const needs = script.needs ?? {};
  const missing = [needs.yapCode && !lab.yapCode ? '--yap-code on' : '', needs.web && !lab.web ? '--web on' : '', needs.connectors && !lab.connectors ? '--connectors on' : ''].filter(Boolean);
  if (missing.length) {
    out.failures.push({ step: 0, error: `This script needs the lab started with ${missing.join(' ')}: npm run -s lab -- stop, then start ${missing.join(' ')}` });
    return out;
  }
  if (script.todo) out.warnings.push({ step: 0, warning: `TODO: ${script.todo}` });
  let last = null;
  let agents = null;
  for (const [i, step] of script.steps.entries()) {
    const n = i + 1;
    const kind = kindOf(step);
    const rec = { n, step: kind, ok: true, ...(step.note && kind !== 'note' ? { note: step.note } : {}) };
    const started = Date.now();
    const soft = !!step.soft;
    try {
      if (kind === 'note') {
        rec.note = step.note;
      } else if (kind === 'fake') {
        if (!lab.fake) rec.skipped = 'not a --fake lab';
        else must(await send('fake', { script: JSON.stringify(step.fake) }));
      } else if (kind === 'ask') {
        const shots = step.shots ?? options.shots;
        const r = must(
          await send('ask', {
            text: step.ask,
            spoken: !!step.spoken,
            chat: step.chat ?? 'new',
            timeout: step.timeout ?? 600,
            ...(step.cards ? { cards: step.cards } : {}),
            ...(shots === false ? { 'no-shots': true } : {}),
            ...((step.theme ?? options.theme) ? { theme: step.theme ?? options.theme } : {}),
          })
        );
        last = r;
        out.turns.push(r.turn);
        Object.assign(rec, { turn: r.turn, status: r.status, conversation: r.conversation, tools: r.tools.map((t) => t.name), shots: (r.shots ?? []).length, ms: r.ms });
        if (r.status === 'running') out.warnings.push({ step: n, warning: `turn ${r.turn} was still answering after ${step.timeout ?? 600} s` });
      } else if (kind === 'answer') {
        // The cards waiting now (a turn's, an agent's): the lab's state has them.
        const id = typeof step.answer === 'string' && step.answer !== 'next' ? step.answer : cardFor(step.answer, (await send('state', {})).pending ?? []);
        const r = must(await send('answer', { _: [id, step.how ?? 'allow'], timeout: step.timeout ?? 600 }));
        rec.answered = r.answered;
        if (r.turn !== undefined) last = r;
        if (r.agents) agents = r;
      } else if (kind === 'wait') {
        if (step.wait === 'agents') {
          agents = must(await send('wait', { agents: true, timeout: step.timeout ?? 600, ...(step.cards ? { cards: step.cards } : {}), ...((step.shots ?? options.shots) === false ? { 'no-shots': true } : {}) }));
          rec.agents = (agents.agents ?? []).map((a) => `${a.id}: ${a.status}`);
          if (agents.autoAnswered) rec.answered = agents.autoAnswered.map((c) => `${c.id} ${c.answer}`);
        } else {
          last = must(await send('wait', { timeout: step.timeout ?? 600 }));
          Object.assign(rec, { turn: last.turn, status: last.status });
        }
      } else if (kind === 'set') {
        rec.set = must(await send('set', step.set));
      } else if (kind === 'shot') {
        const r = await send('shot', { window: step.window ?? 'thread', name: step.shot || 'shot', ...((step.theme ?? options.theme) ? { theme: step.theme ?? options.theme } : { theme: 'both' }), ...(step.selector ? { selector: step.selector } : {}) });
        if (!r.ok) {
          if (step.optional) rec.skipped = r.error;
          else throw new Error(r.error);
        } else rec.shots = (r.shots ?? []).map((s) => s.file);
      } else if (kind === 'expect') {
        const e = step.expect;
        if (e.agent !== undefined && !agents) agents = must(await send('agents', {}));
        const checks = checkTurn(e, { result: last, calls: callsOf(last), agents });
        for (const sc of screenChecks(e)) {
          const got = await send('probe', sc.args);
          checks.push(got.ok ? { check: sc.check, pass: probePasses(sc.want, got), ...(probePasses(sc.want, got) ? {} : { detail: JSON.stringify({ count: got.count, visible: got.visible, has: got.has }) }) } : { check: sc.check, pass: false, detail: got.error });
        }
        rec.checks = checks;
        const missed = checks.filter((x) => !x.pass);
        // A soft step's misses are warnings: the step still passes.
        rec.ok = !missed.length || soft || !!e.soft;
        if (missed.length && rec.ok) rec.warned = true;
        for (const c of missed) {
          const item = { step: n, check: c.check, detail: c.detail, ...(step.note ? { note: step.note } : {}) };
          if (soft || e.soft) out.warnings.push({ ...item, soft: true });
          else out.failures.push(item);
        }
      }
    } catch (err) {
      rec.ok = false;
      rec.error = err.message.split('\n')[0];
      if (soft) out.warnings.push({ step: n, warning: rec.error });
      else out.failures.push({ step: n, error: rec.error });
      if (STOPS.has(kind) && !soft) {
        rec.ms ??= Date.now() - started;
        out.steps.push(rec);
        out.stopped = { step: n, why: `${kind} failed: the steps after it need it` };
        break;
      }
    }
    rec.ms ??= Date.now() - started;
    out.steps.push(rec);
  }
  out.ok = out.failures.length === 0;
  return out;
}

function must(r) {
  if (!r?.ok) throw new Error(r?.error ?? 'the lab said no');
  return r;
}
