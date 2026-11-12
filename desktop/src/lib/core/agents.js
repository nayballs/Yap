// Yap Code (desktop/src-tauri/src/agents.rs): Chat or the Voice Assistant hands a
// task to the person's own Claude Code, in a folder they chose. These are
// the words Yap Code's screens share — an agent's status, what it costs and how
// it's paid, how long it has run, its timeline from its log (steps and what
// it said between them), the line it's on now, Claude Code's to-do list,
// what changed in each file and the commands it ran, the pills under the
// answer that started it, a problem in plain words with its one button —
// and the Mode chip's keys. No Svelte state: pure, tested by
// desktop/e2e/agents-ui.spec.js.
import { dollars } from './money.js';
import { FEATURE_NAME } from './codeChats.js';
// What the main window's chunk needs at start-up (stores/agents.svelte.js)
// lives in agentNow.js, so the rest of this file loads with the views that
// show agents (frontend.md, "One chunk per window").
import { isLive, PLAN_WAITING, PLAN_WAITS_S, planWaitsNow, latestStep } from './agentNow.js';
export { isLive, PLAN_WAITING, PLAN_WAITS_S, planWaitsNow, latestStep };

/** A plan nobody went ahead with before a newer turn began (or set aside by the person): its row's line. */
export const PLAN_NOT_USED = 'Plan not used';

/** An agent's status as the screens say it. */
export const STATUS_WORDS = {
  queued: 'Waiting to start',
  running: 'Running',
  asking: 'Waiting for you',
  done: 'Done',
  failed: "Didn't finish",
  stopped: 'Stopped',
  interrupted: 'Interrupted',
};

/** The status in words ('' for none). */
export const statusWord = (status) => STATUS_WORDS[status] || (status ? String(status) : '');

/** The status dot's tone: live (working), ask (waiting for you), done, failed, idle (stopped, interrupted, queued). */
export function statusTone(status) {
  if (status === 'running') return 'live';
  if (status === 'asking') return 'ask';
  if (status === 'done') return 'done';
  if (status === 'failed') return 'failed';
  return 'idle';
}

/**
 * **Undo agent is offered**: it's over, it changed files, and their backups
 * are there — not undone already, not swept (7 days on), and made at all
 * (`noBackups`: Claude Code's hooks didn't run, so nothing was backed up).
 */
export const canUndo = (agent) => !!agent && !isLive(agent.status) && (agent.files?.length ?? 0) > 0 && !agent.undone && !agent.swept && !agent.noBackups;

/** The agent's tab when it changed files with no backups made. */
export const NO_BACKUPS = "No backups were made, so Undo can't put these files back.";

/** Dollars as an answer's byline says them (core/money.js): "$0.04", "$0.0042", "under $0.0001". */
export const money = (usd) => dollars((Number(usd) || 0) * 1_000_000);

/**
 * **How an agent is paid, and what it cost**, told apart — in the words of a
 * plan chat's answers (core/aiModel.js `paidLine`): on the person's Claude
 * plan the cost is only what the API would have charged ("Your Claude plan
 * · ≈ $0.0042 on the API"; Claude Code's estimate); on their API key it's
 * what it cost ("API key · $0.0042"). Nothing yet: the way alone.
 */
export function costLine(agent) {
  const cost = Number(agent?.cost) || 0;
  const way = agent?.pay === 'key' ? 'API key' : 'Your Claude plan';
  if (!(cost > 0)) return way;
  return agent?.pay === 'key' ? `API key · ${money(cost)}` : `Your Claude plan · ≈ ${money(cost)} on the API`;
}

/** Seconds as the screens say a length of time: "12 s", "3 min", "1 h 5 min". */
export function duration(secs) {
  const s = Math.max(0, Math.round(Number(secs) || 0));
  if (s < 60) return `${s} s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m} min`;
  const h = Math.floor(m / 60);
  const rest = m % 60;
  return rest ? `${h} h ${rest} min` : `${h} h`;
}

/**
 * How long an agent has run (`now`: unix seconds): from its first start to
 * now while it isn't over, else to its latest turn's end. '' before it ever
 * started.
 */
export function elapsed(agent, now = Date.now() / 1000) {
  const from = Number(agent?.startedTs) || 0;
  if (!from) return '';
  const to = isLive(agent.status) ? now : Number(agent.endedTs) || Number(agent.updatedTs) || now;
  return duration(to - from);
}

/**
 * An agent's steps from its log (`yap-agent` events, oldest first): each line
 * once in a row, with the file it changes or the command it runs (and what
 * Claude Code said it's for). `[{step, file, command, about, ts}]`.
 */
export function stepsOf(log) {
  return timelineOf(log).filter((t) => t.kind === 'step');
}

/** What a log event says Claude Code said as it worked: `said`, or (an older Yap's log) a `text` while it ran that wasn't its result's. */
function saidOf(e) {
  if (typeof e?.said === 'string') return e.said.trim();
  if (typeof e?.text === 'string' && !e.step && e.cost == null && isLive(e.status)) return e.text.trim();
  return '';
}

/** A step's line once it's done: "Editing notes.md" → "Edited notes.md". */
const DONE = [
  [/^Editing\b/, 'Edited'],
  [/^Writing\b/, 'Wrote'],
  [/^Running\b/, 'Ran'],
  [/^Reading\b/, 'Read'],
  [/^Looking\b/, 'Looked'],
  [/^Searching\b/, 'Searched'],
  [/^Starting\b/, 'Started'],
  [/^Picking up\b/, 'Picked up'],
  [/^Waiting\b/, 'Waited'],
  [/^Using\b/, 'Used'],
  [/^Handing\b/, 'Handed'],
];
const doneLine = (step) => DONE.reduce((s, [was, now]) => (was.test(s) ? s.replace(was, now) : s), String(step || ''));

/**
 * **A picture a step's tool call came back with** (the log's `picture`
 * event, agents/pictures.rs: a picture file read, a screenshot a tool took):
 * `{ id, w, h }` as Chat's pictures are, its id the address the `yapsnip`
 * scheme serves it by (`agent/<agent id>/<img id>`: chatImages.js
 * `chatImageUrl`). `null`: the event isn't one.
 */
export function stepPicture(e) {
  const p = e?.picture;
  if (!p?.id || !e?.id) return null;
  return { id: `agent/${e.id}/${p.id}`, w: Number(p.w) || 0, h: Number(p.h) || 0 };
}

/**
 * A step as the lists keep it: its line, its file or command, the tool calls
 * it stands for (`calls`: the same line twice in a row is one step) and
 * their pictures; a command's line once done (`done`), the lines a Read
 * asked for (`range`); then, as they come back, what its edit changed
 * (`diff`) and what its command printed (`output`).
 */
const stepOf = (e, step) => ({
  kind: 'step',
  step,
  file: e.file || null,
  command: e.command || null,
  about: e.about || null,
  done: e.done || null,
  range: e.range || null,
  calls: e.call ? [e.call] : [],
  pictures: [],
  diff: null,
  output: null,
  // A helper's own step: the call that handed it the work (`AgentEvent::under`).
  under: e.under || null,
  // A helper's own steps, when this step handed part of the work over (`workOf`).
  helped: [],
  ts: Number(e.ts) || 0,
});

/** Every step of a work list, a helper's own after the step that sent it. */
const everyStep = (out) => out.flatMap((x) => (x.kind === 'work' ? x.steps.flatMap((st) => [st, ...st.helped]) : []));

/** `diff` (an event's) on its step among `steps`: the one that stands for its call, else (an older log's) the earliest that changes that file and has none yet. */
function addDiff(steps, call, diff) {
  const at = (call && steps.findLast((x) => x.calls.includes(call))) || steps.find((x) => x.file === diff.file && !x.diff);
  if (at && !at.diff) at.diff = diff;
}

/** What a command printed (an event's `output`) on its step: the one that stands for its call. */
function addOutput(steps, call, output) {
  const at = call && steps.findLast((x) => x.calls.includes(call));
  if (at && !at.output) at.output = output;
}

/** `pic` (an event's, `stepPicture`) on its step among `steps`: the newest that stands for its call, else the newest; once. Its caption is that step's line, as done. */
function addPicture(steps, call, pic) {
  const at = (call && steps.findLast((x) => x.calls.includes(call))) || steps.at(-1);
  if (at && !at.pictures.some((p) => p.id === pic.id)) at.pictures.push({ ...pic, caption: doneLine(at.step) });
}

/**
 * **An agent's timeline** from its log, oldest first: its steps (each line
 * once in a row) and what it said between them, in order —
 * `{kind: 'step', step, file, command, about, calls, pictures, ts}` or
 * `{kind: 'said', text, ts}`. `final`: its final words, shown apart, so the
 * same words at the end aren't said twice.
 */
export function timelineOf(log, final = '') {
  const out = [];
  for (const e of log || []) {
    const pic = stepPicture(e);
    if (pic) {
      addPicture(out.filter((x) => x.kind === 'step'), e.call, pic);
      continue;
    }
    const said = saidOf(e);
    if (said) {
      out.push({ kind: 'said', text: said, ts: Number(e.ts) || 0 });
      continue;
    }
    const step = typeof e?.step === 'string' ? e.step.trim() : '';
    if (!step) continue;
    const last = out[out.length - 1];
    if (last?.kind === 'step' && last.step === step && !e.file && !e.command) {
      if (e.call) last.calls.push(e.call);
      continue;
    }
    out.push(stepOf(e, step));
  }
  const end = String(final || '').trim();
  if (end) {
    for (let i = out.length - 1; i >= 0; i--) {
      if (out[i].kind === 'said' && out[i].text === end) {
        out.splice(i, 1);
        break;
      }
      if (out[i].kind === 'said') break;
    }
  }
  return out;
}

/**
 * **An agent's work as the thread tells it** (11 Nov 2026, after Claude
 * Code's own transcript): what it said, in order, and between its words one
 * group for each stretch of steps — `{kind: 'said', text, ts}` or `{kind:
 * 'work', steps: [{step, file, command, about, calls, pictures, ts}], edits:
 * [{file, added, removed, made}], ts}` — so a long run of tool calls reads
 * as one line ("Ran 3 commands, edited convert.js +4 −1", `workSummary`)
 * that opens into its steps; a step's `pictures` are the ones its tool call
 * came back with (`groupPictures`: its line's), and its `helped` the steps of
 * a helper it handed part of the work to (folded behind its row, never
 * counted in the line). `final`: its final words,
 * left out (its report tells them), as
 * `timelineOf` leaves them. A group of housekeeping alone ("Starting Claude
 * Code") is kept only at the end of a turn still working (`live`), where
 * it's what the agent is on.
 */
export function workOf(log, final = '', live = false) {
  const out = [];
  let group = null;
  for (const e of log || []) {
    // A picture its call came back with: on that step, in whichever stretch.
    const pic = stepPicture(e);
    if (pic) {
      addPicture(everyStep(out), e.call, pic);
      continue;
    }
    // What its command printed: on that step too.
    if (e?.output && typeof e.output.text === 'string') {
      addOutput(everyStep(out), e.call, e.output);
      continue;
    }
    const said = saidOf(e);
    if (said) {
      group = null;
      out.push({ kind: 'said', text: said, ts: Number(e.ts) || 0 });
      continue;
    }
    const step = typeof e?.step === 'string' ? e.step.trim() : '';
    // A helper's own step (`under`: the call that handed it the work, agents/runner.rs): folded behind
    // that call's row, never a step of the session's own stretch — as Claude Code's transcript keeps a
    // subagent's work out of the session's. (No such row here — an older log —: a step like any other.)
    const sender = step && e.under ? everyStep(out).findLast((x) => x.calls.includes(e.under)) : null;
    if (sender) {
      const last = sender.helped[sender.helped.length - 1];
      if (last && last.step === step && !e.file && !e.command) {
        if (e.call) last.calls.push(e.call);
      } else {
        sender.helped.push(stepOf(e, step));
      }
    } else if (step) {
      if (!group) {
        group = { kind: 'work', steps: [], edits: [], ts: Number(e.ts) || 0 };
        out.push(group);
      }
      const last = group.steps[group.steps.length - 1];
      if (last && last.step === step && !e.file && !e.command) {
        if (e.call) last.calls.push(e.call);
      } else {
        group.steps.push(stepOf(e, step));
      }
    }
    // What an edit changed, once it went through: its step's, and its stretch's (a helper's edit is its
    // row's alone: the stretch's line counts the session's own).
    const d = e?.diff;
    if (d?.file) {
      addDiff(everyStep(out), e.call, d);
      const owner = e.call ? everyStep(out).findLast((x) => x.calls.includes(e.call)) : null;
      const helpers = !!owner && !out.some((x) => x.kind === 'work' && x.steps.includes(owner));
      const g = helpers ? null : (group ?? out.filter((x) => x.kind === 'work').at(-1));
      if (g) {
        let f = g.edits.find((x) => x.file === d.file);
        if (!f) g.edits.push((f = { file: d.file, added: 0, removed: 0, made: d.kind === 'new' ? 'new' : null }));
        f.added += Number(d.added) || 0;
        f.removed += Number(d.removed) || 0;
      }
    }
  }
  const end = String(final || '').trim();
  if (end) {
    for (let i = out.length - 1; i >= 0; i--) {
      if (out[i].kind !== 'said') continue;
      if (out[i].text === end) out.splice(i, 1);
      break;
    }
  }
  // (Yap's own housekeeping alone — "Starting Claude Code", a note read, a denial — isn't a stretch of work, unless a step of it came back with a picture.)
  const real = (g) => g.steps.some((s) => s.pictures.length > 0 || !['other', 'ask'].includes(stepKind(s)));
  return out.filter((x, i) => x.kind === 'said' || real(x) || (live && i === out.length - 1));
}

const fileName = (f) => String(f || '').split(/[\\/]/).pop();
const some = (n, one, many) => (n === 1 ? one : `${n} ${many}`);

/**
 * **A stretch of work in a line** (`workOf`'s group), as Claude Code's
 * transcript says one: `[{text, added?, removed?}]`, read with ", " between
 * ("Ran 3 commands", "read a file", "used github 2 times", "edited
 * convert.js" +4 −1). Other tools' calls (an MCP server's, a connector's, a
 * skill) are named when they're all one's, else counted ("used 3 tools"); a
 * part handed to a subagent is "handed part of it to a helper". One real
 * step: that step's own line, as done ("Read notes.md", "Used github ·
 * create issue"). `busy`: the agent is on it now — its latest step, as it is
 * ("Running npm test").
 */
export function workSummary(group, busy = false) {
  const steps = group?.steps || [];
  const edits = group?.edits || [];
  if (!steps.length) return [];
  if (busy) return [{ text: steps[steps.length - 1].step }];
  const kinds = steps.map(stepKind);
  const real = steps.filter((_, i) => !['other', 'ask'].includes(kinds[i]));
  if (real.length <= 1) {
    const s = real[0] ?? steps[steps.length - 1];
    const e = s.file ? edits.find((x) => x.file === s.file) : edits.length === 1 && stepKind(s) === 'edit' ? edits[0] : null;
    return [{ text: doneLine(s.step), ...(e ? { added: e.added, removed: e.removed } : {}) }];
  }
  const count = (k) => kinds.filter((x) => x === k).length;
  const parts = [];
  const add = (text, counts = null) => parts.push({ text: parts.length ? text : text[0].toUpperCase() + text.slice(1), ...(counts ? { added: counts.added, removed: counts.removed } : {}) });
  const ran = count('command');
  if (ran) add(`ran ${some(ran, 'a command', 'commands')}`);
  const read = count('read');
  if (read) add(`read ${some(read, 'a file', 'files')}`);
  const searched = count('search');
  if (searched) add(searched === 1 ? 'searched the files' : `searched the files ${searched} times`);
  const web = count('web');
  if (web) add(web === 1 ? 'looked on the web' : `looked on the web ${web} times`);
  // Other tools' calls: whose, when they're all one's ("used github 3 times"), else how many.
  const used = steps.filter((_, i) => kinds[i] === 'tool').map(toolOwner);
  if (used.length) {
    const one = used.every((u) => u && u === used[0]) ? used[0] : '';
    add(one ? (used.length === 1 ? `used ${one}` : `used ${one} ${used.length} times`) : `used ${used.length} tools`);
  }
  const helpers = count('helper');
  if (helpers) add(helpers === 1 ? 'handed part of it to a helper' : `handed parts of it to ${helpers} helpers`);
  const files = [...new Set([...steps.filter((s, i) => kinds[i] === 'edit' && s.file).map((s) => s.file), ...edits.map((e) => e.file)])];
  const edited = count('edit');
  if (files.length === 1) {
    const e = edits.find((x) => x.file === files[0]);
    add(`${e?.made === 'new' ? 'created' : 'edited'} ${fileName(files[0])}`, e);
  } else if (files.length > 1) {
    add(`edited ${files.length} files`, { added: edits.reduce((n, e) => n + e.added, 0), removed: edits.reduce((n, e) => n + e.removed, 0) });
  } else if (edited) {
    add(`made ${some(edited, 'an edit', 'edits')}`);
  }
  // Commands that failed, as Claude Code's own line says them ("ran 5 commands, …, 2 failed").
  const failed = steps.filter((x) => x.output?.failed).length;
  if (failed) add(`${failed} failed`);
  return parts;
}

/**
 * **A step as its row says it** (a stretch opened, after Claude Code's own
 * transcript): `{ text, name?, tail?, added?, removed?, failed? }` — its
 * words, the file's name set apart ("Edited" + "agents.js", "Created" +
 * "notes.md", "Read" + "app.ts" + "(424–503)"), an edit's lines added and
 * removed, a command as done ("Checked the edits parse": Rust's `done`, else
 * its line) and whether it failed. `live`: the step the agent is on now, as
 * it is ("Running npm test").
 */
export function stepRow(s, live = false) {
  if (live) return { text: String(s?.step || '') };
  const kind = stepKind(s);
  if (kind === 'edit' && s.file) {
    const verb = s.diff?.kind === 'new' ? 'Created' : /^Writing\b/.test(s.step) ? 'Wrote' : 'Edited';
    return { text: verb, name: fileName(s.file), ...(s.diff ? { added: s.diff.added, removed: s.diff.removed } : {}) };
  }
  const read = kind === 'read' ? /^Reading (.+)$/.exec(s.step) : null;
  if (read && read[1] !== 'a file') return { text: 'Read', name: fileName(read[1]), ...(s.range ? { tail: `(${s.range})` } : {}) };
  if (kind === 'command') return { text: s.done || doneLine(s.step), ...(s.output?.failed ? { failed: true } : {}) };
  return { text: doneLine(s.step) };
}

/** Yap's and Claude Code's own plumbing among a stretch's steps: no row of its own once it's past. */
const PLUMBING = /^(Starting Claude Code|Picking up your request|Picking up where it left off|Finding a tool|Using yap · approve)$/;

/**
 * **The rows of a stretch opened**: `[{ step, i, now }]` — its steps in
 * order (`i`: its place among them), plumbing left out ("Starting Claude
 * Code", "Finding a tool") unless it's the one the agent is on (`now`: the
 * last, while `busy`) or it came back with a picture.
 */
export function rowSteps(steps, busy = false) {
  const list = steps || [];
  return list.map((step, i) => ({ step, i, now: busy && i === list.length - 1 })).filter((r) => r.now || r.step.pictures?.length > 0 || !PLUMBING.test(String(r.step.step || '')));
}

/** Whether a step's row opens: it has a diff, a command (and what it printed), a picture, or a helper's own steps to show. */
export const stepOpens = (s) => !!(s?.diff?.lines?.length || s?.command || s?.pictures?.length || s?.helped?.length);

/**
 * **A diff's lines as numbered rows** (the thread's row, as Claude Code's
 * transcript numbers them): `[{ mark: 'del' | 'add' | 'ctx' | 'gap', text,
 * no }]` — `no` a removed line's number in the file before, any other
 * line's after, when its place is known: under a `@@ -a,b +c,d @@` line
 * (agents/diff.rs `of_patch`: Claude Code's own patch), or a file written
 * whole (from 1). A place line itself is a gap between two parts, none
 * before the first.
 */
export function diffRows(diff) {
  const lines = diff?.lines || [];
  const out = [];
  let before = null;
  let after = null;
  if (['new', 'rewrite', 'write'].includes(diff?.kind) && !lines.some((l) => String(l).startsWith('@'))) before = after = 1;
  for (const l of lines) {
    const s = String(l ?? '');
    const at = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/.exec(s);
    if (at || s === '@') {
      if (out.length) out.push({ mark: 'gap', text: '', no: null });
      before = at ? Number(at[1]) : null;
      after = at ? Number(at[2]) : null;
      continue;
    }
    const x = diffLine(s);
    if (x.mark === 'del') out.push({ ...x, no: before == null ? null : before++ });
    else if (x.mark === 'add') out.push({ ...x, no: after == null ? null : after++ });
    else {
      out.push({ ...x, no: after });
      if (before != null) before++;
      if (after != null) after++;
    }
  }
  return out;
}

/** **A stretch's pictures** (`workOf`'s group): its steps', in order — the small ones at the end of its line. */
export const groupPictures = (group) => (group?.steps || []).flatMap((x) => x.pictures || []);

/**
 * A step's picture as the panel's picture tab and the viewer take it (a
 * `ShownPicture`, as shares.js `asShownPicture`): its step's line as its
 * caption ("Read shot.png"), the agent's workspace as where it's from.
 */
export function shownStepPicture(pic, folder = '') {
  return { id: pic?.id, w: pic?.w || 0, h: pic?.h || 0, caption: pic?.caption || 'Picture', alt: '', imageUrl: '', page: { title: folder || '', site: folder || '', url: '' }, n: 0 };
}

/** Narration long enough to fold (its first lines shown, "Show more"). */
export const longSaid = (text) => String(text || '').length > 280 || String(text || '').split('\n').length > 4;

/** **Claude Code's to-do list**, as its latest `TodoWrite` left it: `[{content, status, activeForm}]` (status pending | in_progress | completed). */
export function todosOf(log) {
  for (let i = (log?.length ?? 0) - 1; i >= 0; i--) {
    if (Array.isArray(log[i]?.todos)) return log[i].todos.filter((t) => t && typeof t.content === 'string');
  }
  return [];
}

/** "2 of 5 done". */
export function todoCount(todos) {
  const list = todos || [];
  return `${list.filter((t) => t.status === 'completed').length} of ${list.length} done`;
}

/** **What changed in each file**, from the log's diffs (`diff.rs`): a Map from the file (as the agent's files say it) to its diffs, in order. */
export function diffsOf(log) {
  const out = new Map();
  for (const e of log || []) {
    const d = e?.diff;
    if (!d?.file || !Array.isArray(d.lines)) continue;
    out.set(d.file, [...(out.get(d.file) || []), d]);
  }
  return out;
}

/** A file's diffs added up: `{added, removed, made}` — `made` "new" when the agent made it (its first write a new file). */
export function diffTotals(diffs) {
  let added = 0;
  let removed = 0;
  for (const d of diffs || []) {
    added += Number(d.added) || 0;
    removed += Number(d.removed) || 0;
  }
  return { added, removed, made: diffs?.[0]?.kind === 'new' ? 'new' : null };
}

const lines = (n) => (n === 1 ? '1 line' : `${n.toLocaleString('en-GB')} lines`);

/** A written file's heading: "New file · 12 lines", "Rewritten · 12 lines", "Written · 12 lines"; '' for an edit. */
export function diffTitle(d) {
  const n = Number(d?.added) || 0;
  if (d?.kind === 'new') return `New file · ${lines(n)}`;
  if (d?.kind === 'rewrite') return `Rewritten · ${lines(n)}`;
  if (d?.kind === 'write') return `Written · ${lines(n)}`;
  return '';
}

/** "…12 more lines" past a diff's caps ('' for none). */
export const moreLines = (n) => (Number(n) > 0 ? `…${Number(n).toLocaleString('en-GB')} more ${Number(n) === 1 ? 'line' : 'lines'}` : '');

/** A diff line's mark and text: `{mark: 'del' | 'add' | 'ctx' | 'gap', text}` (a part's place in the file, `@@ -a,b +c,d @@`, is a gap too: `diffRows` numbers from it). */
export function diffLine(l) {
  const s = String(l ?? '');
  if (s === '@' || s.startsWith('@@ ')) return { mark: 'gap', text: '' };
  const m = s.charAt(0);
  return { mark: m === '-' ? 'del' : m === '+' ? 'add' : 'ctx', text: s.slice(1) };
}

/**
 * **The commands it ran**, each with what Claude Code said it's for:
 * `[{command, about}]` — the agent's own list (`agent.commands`), with the
 * descriptions its log's events carry, in order.
 */
export function commandsOf(agent, log) {
  const told = (log || []).filter((e) => typeof e?.command === 'string');
  return (agent?.commands || []).map((command, i) => ({ command, about: told[i]?.command === command ? told[i].about || '' : '' }));
}

/** The message index a chat message id names ("chat:12:4" → 4), or -1. */
export function messageIndex(id, conversation) {
  const m = /^chat:(\d+):(\d+)$/.exec(String(id || ''));
  if (!m) return -1;
  if (conversation != null && Number(m[1]) !== Number(conversation)) return -1;
  return Number(m[2]);
}

/**
 * **A request in a few words** (a turn's header; agents/store.rs `title_of`):
 * its first line to its first sentence's end, cut at a word under 60
 * characters.
 */
export function titleOf(text) {
  const line = (String(text || '').trim().split('\n').map((l) => l.trim()).find(Boolean) || '').replace(/^[#\-*> ]+/, '');
  const end = line.search(/[.!?:;](\s|$)/);
  const sentence = (end >= 0 ? line.slice(0, end) : line).trim();
  if ([...sentence].length <= 60) return sentence;
  const cut = [...sentence].slice(0, 60).join('');
  const at = cut.lastIndexOf(' ');
  return `${(at > 30 ? cut.slice(0, at) : cut).replace(/[, -]+$/, '')}…`;
}

/**
 * **An agent's turns** (agents/store.rs `TurnNote`: 2026-11-07, one agent per Code
 * chat): each request to it and what came of it — `{n, title, request,
 * status, startedTs, endedTs, cost, files, denials, text, plan, asks, error,
 * undone, messageId, reportMessageId, events}` — its log split among them
 * (each event's `turn`; an event without one goes with the turn before it,
 * an older log's all with its first). An agent from before turns: one, made
 * from the agent. An event naming a turn this window hasn't read yet gets a
 * turn of its own until it does.
 */
export function turnsOf(agent, log) {
  if (!agent) return [];
  const notes = agent.turnList?.length
    ? agent.turnList
    : [
        {
          n: 1,
          title: titleOf(agent.brief),
          request: agent.brief || '',
          status: agent.status,
          startedTs: agent.startedTs,
          endedTs: agent.endedTs,
          cost: agent.cost,
          files: agent.files || [],
          denials: agent.denials || [],
          text: agent.text || '',
          plan: agent.plan,
          planAllowed: agent.planAllowed,
          planUnused: agent.planUnused,
          asks: agent.asks,
          error: agent.error,
          undone: agent.undone,
          messageId: agent.messageId,
          reportMessageId: agent.reportMessageId,
        },
      ];
  const events = new Map(notes.map((t) => [t.n, []]));
  const extra = new Map();
  let at = notes[0]?.n ?? 1;
  for (const e of log || []) {
    if (Number.isInteger(e?.turn)) at = e.turn;
    if (!events.has(at) && !extra.has(at)) extra.set(at, { n: at, title: e?.queued || '', request: '', status: e?.status, files: [], denials: [], text: '' });
    (events.get(at) ?? (events.set(at, []), events.get(at))).push(e);
  }
  return [...notes, ...[...extra.values()].sort((a, b) => a.n - b.n)].map((t) => ({ ...t, files: t.files || [], denials: t.denials || [], events: events.get(t.n) || [] }));
}

/**
 * **How long an agent has worked**, its turns added up (the time between them
 * left out: a follow-up asked hours later isn't hours of work); an agent from
 * before turns, `elapsed`. '' before it ever started.
 */
export function workedFor(agent, now = Date.now() / 1000) {
  const turns = (agent?.turnList || []).filter((t) => Number(t.startedTs) > 0);
  if (turns.length < 2) return elapsed(agent, now);
  const last = turns.at(-1);
  const secs = turns.reduce((sum, t) => {
    const to = t === last && isLive(agent.status) ? now : Number(t.endedTs) || Number(t.startedTs);
    return sum + Math.max(0, to - Number(t.startedTs));
  }, 0);
  return duration(secs);
}

/** The turn an agent is on now (the newest that ran), for how long it has been at it: the running list's time. */
export const turnNow = (agent) => [...(agent?.turnList || [])].reverse().find((t) => Number(t.startedTs) > 0) ?? null;

/** A turn's status as its header says it: queued behind the one working, never sent (stopped first), else the agent's words. */
export function turnStatusWord(t) {
  if (t?.status === 'queued' && !t.startedTs) return 'Queued';
  if ((t?.status === 'stopped' || t?.status === 'interrupted') && !t.startedTs) return 'Not sent yet';
  return statusWord(t?.status);
}

/** **Undo this turn** is offered: the agent is over, the turn changed files, and their backups are there (as `canUndo`). */
export const canUndoTurn = (agent, turn) => !!agent && !!turn && !isLive(agent.status) && (turn.files?.length ?? 0) > 0 && !turn.undone && !agent.swept && !agent.noBackups;

/** The answer after message `q`: the first assistant message before the person's next (a report marker skipped), or -1. */
function answerAfter(messages, q) {
  for (let i = q + 1; i < messages.length; i++) {
    if (messages[i]?.kind === 'agentReport') continue;
    if (messages[i]?.role === 'assistant') return i;
    if (messages[i]?.role === 'user') return -1;
  }
  return -1;
}

/**
 * **The agent pills under each answer**: each turn of the agents of
 * `conversation` under the first answer after the question that asked for
 * it (its `messageId`; an agent from before turns: the agent's), one pill an agent
 * an answer (its newest turn there) — `{...agent, turn, turnStatus,
 * turnStarted, turnTitle}`. `messages`: the thread's (`role`). A Map from the answer's
 * index to its pills, oldest first.
 */
export function pillsByAnswer(agents, conversation, messages) {
  return pillsAt(agents, conversation, messages, false);
}

/**
 * **A turn's end, from its own events** (12 Nov 2026): `{over, text}` —
 * whether its latest event says it's over, and then its final words (its
 * latest event with `text`). The thread hears a turn's last event at once;
 * the store's record of the turn follows a moment later (it's read again),
 * so a turn's work goes by this meanwhile: no spinner on a turn that has
 * ended, and its final words known as soon as they're said.
 */
export function turnEnd(events) {
  const list = events || [];
  const last = list.length ? list[list.length - 1] : null;
  const over = !!last && !isLive(last.status);
  const said = over ? list.findLast((e) => typeof e?.text === 'string' && e.text.trim()) : null;
  return { over, text: said ? said.text.trim() : '' };
}

/**
 * **Is the answer to the question at `i` in the thread?** In a Code chat
 * that works directly the session's final words become the chat's answer:
 * an assistant message after the question, before the next message that
 * started a turn of its own (`startsTurn(k)`). Until it's there the words
 * stay where they were written, in its work; from then they're the
 * answer's alone — never shown twice, never gone for a moment. Pure.
 */
export function answeredAfter(messages, i, startsTurn = () => false) {
  const list = messages || [];
  for (let k = i + 1; k < list.length; k++) {
    if (startsTurn(k)) return false;
    const m = list[k];
    if (m?.role === 'assistant' && !m.kind && !m.streaming) return true;
  }
  return false;
}

/**
 * **The Code chat's own session, at work** (12 Nov 2026): the agent of
 * `conversation` still going whose latest turn is the person's own message
 * (`direct`: a Code chat that works directly) — what the message box's ■
 * stops, as Claude Code's own app has Stop where Send was while it works.
 * Null: none (an agent Chat sent out is stopped from its tab, as ever).
 */
export function workingDirectly(agents, conversation) {
  if (conversation == null) return null;
  return (agents || []).find((a) => a.conversation === conversation && isLive(a.status) && !!a.turnList?.at(-1)?.direct) ?? null;
}

/**
 * **The agent pills under a question** (11 Nov 2026): the turns the person's
 * own message started in a Code chat that works directly (a turn's
 * `direct`: agents/direct.rs). There's no answer of Chat's to put them
 * under — its session's work shows under the message itself, and its own
 * words come after. A Map from the question's index to its pills, as
 * `pillsByAnswer`'s.
 */
export function pillsByQuestion(agents, conversation, messages) {
  return pillsAt(agents, conversation, messages, true);
}

function pillsAt(agents, conversation, messages, direct) {
  const out = new Map();
  if (conversation == null || !messages?.length) return out;
  const mine = (agents || []).filter((j) => j?.conversation === conversation).sort((a, b) => (a.createdTs || 0) - (b.createdTs || 0));
  for (const j of mine) {
    const turns = j.turnList?.length ? j.turnList : [{ n: 1, messageId: j.messageId, status: j.status, title: '' }];
    for (const t of turns) {
      if (!!t.direct !== direct) continue;
      const q = messageIndex(t.messageId, conversation);
      if (q < 0) continue;
      const at = direct ? (q < messages.length ? q : -1) : answerAfter(messages, q);
      if (at < 0) continue;
      const pill = { ...j, turn: t.n, turnStatus: t.status, turnStarted: Number(t.startedTs) || 0, turnTitle: t.title || '' };
      const list = out.get(at) || [];
      const k = list.findIndex((p) => p.id === j.id);
      if (k >= 0) list[k] = pill;
      else list.push(pill);
      out.set(at, list);
    }
  }
  return out;
}

/**
 * The agent a report marker message is about: the one whose `reportMessageId`
 * it is, or one of whose turns' it is (an agent keeps its latest report's, each
 * turn the report that told it), else — an earlier report from before turns were kept — the
 * newest agent in that chat and folder. null when its agent is gone (the card
 * still says what the marker says).
 */
export function agentOfReport(agents, markerId, conversation, folder = '') {
  const list = agents || [];
  const exact = list.find((j) => j?.reportMessageId && j.reportMessageId === markerId) || list.find((j) => (j?.turnList || []).some((t) => t?.reportMessageId === markerId));
  if (exact) return exact;
  if (!folder) return null;
  return list.filter((j) => j?.conversation === conversation && j.folder === folder).sort((a, b) => (b.createdTs || 0) - (a.createdTs || 0))[0] ?? null;
}

/**
 * The turn of `agent` a report marker is about (`null`: not known, an older
 * agent's): the latest of the turns it covers — a request asked while the
 * agent worked is in the same report as the turn before it (one report when
 * it stops working, agents/runner.rs `reports`), and the card reads as the
 * agent's latest turn then.
 */
export const turnOfReport = (agent, markerId) =>
  (agent?.turnList || []).filter((t) => t?.reportMessageId === markerId).at(-1) ?? null;

/**
 * **An agent tab's name** in the panel's row: "Agent · garden-planner" — and,
 * when another agent tab there has the same folder (a fresh agent beside the
 * chat's earlier one), its first request too, so no two tabs read the
 * same ("Agent · garden-planner · Add a minimap button"; failing that, "#2").
 */
export function agentTabName(t, tabs, lookup = null) {
  const folder = t?.agent?.folder || 'folder';
  const same = (tabs || []).filter((o) => o?.kind === 'agent' && (o.agent?.folder || 'folder') === folder);
  if (same.length < 2) return `Agent · ${folder}`;
  // (A tab opened from a turn's event has the agent's ref only: its turns from
  // the agents list; an agent from before turns, its brief's first words.)
  const first = (o) => {
    const j = lookup?.(o.agent?.id) ?? o.agent;
    return j?.turnList?.[0]?.title || titleOf(j?.brief) || '';
  };
  const title = first(t);
  const named = !!title && same.filter((o) => first(o) === title).length === 1;
  return named ? `Agent · ${folder} · ${title}` : `Agent · ${folder} #${same.indexOf(t) + 1}`;
}

/** A plan the person allowed (ExitPlanMode's Go ahead): its report card's line. */
export const PLAN_ALLOWED = 'The agent is carrying out its plan';

/** The report card's line: "Agent done", "The agent's plan is ready", "The agent is carrying out its plan", "The agent has a question", "Agent didn't finish". */
export function reportTitle(agent, markerText = '') {
  // (An agent at work again since says what the marker said.)
  if (agent && !isLive(agent.status)) {
    if (agent.status === 'failed') return "Agent didn't finish";
    if (agent.plan && agent.planAllowed) return PLAN_ALLOWED;
    if (agent.plan) return "The agent's plan is ready";
    if (agent.asks) return 'The agent has a question';
    if (agent.status === 'stopped') return 'Agent stopped';
    return 'Agent done';
  }
  // From the marker's own words (agents/report.rs `marker_text`).
  if (/: it didn't finish\./.test(markerText)) return "Agent didn't finish";
  if (/: it made a plan, and the user allowed it/.test(markerText)) return PLAN_ALLOWED;
  if (/: it made a plan/.test(markerText)) return "The agent's plan is ready";
  if (/asks the user something/.test(markerText)) return 'The agent has a question';
  return 'Agent done';
}

/**
 * **A report's plan still waits for its go-ahead** (its card's Go ahead,
 * `agent_go_ahead`): the turn it reports is a plan, not allowed already, not
 * set aside ("Plan not used": a newer turn began in its chat, or the person
 * put it aside), it's the agent's latest turn, and the agent isn't working.
 * An agent from before turns: the agent itself, when this is its latest
 * report.
 */
export function planWaits(agent, turn, latest = false) {
  if (!agent || isLive(agent.status) || !agent.plan || agent.planAllowed || agent.planUnused) return false;
  const last = (agent.turnList || []).at(-1);
  if (last?.readOnly) return false;
  if (turn) return !!turn.plan && !turn.planAllowed && !turn.planUnused && !turn.readOnly && !!last && last.n === turn.n;
  return latest && !last;
}

/**
 * **A turn's plan was set aside** ("Plan not used"): it ended as a plan,
 * nobody went ahead with it, and it no longer waits — a newer turn began in
 * its chat, the person put it aside, or a day went by (`waiting`: whether
 * it waits now, the Agent panel's Needs you). Its Go ahead is still there,
 * in its row's details. Pure.
 */
export function planNotUsed(turn, waiting = false) {
  return !!turn && turn.status === 'done' && !!turn.plan && !turn.planAllowed && !turn.readOnly && !waiting;
}

/** The folder a report marker names ("The agent in garden-planner: …"; "The job in …" in a chat from before the rename to Yap Code). */
export function reportFolder(markerText) {
  const m = /^The (?:agent|job) in (.+?): /.exec(String(markerText || ''));
  return m ? m[1] : '';
}

/** A time of day, as the person's PC says it ("4:00 pm"). */
function clock(unixSecs) {
  return new Date(unixSecs * 1000).toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' });
}

/**
 * **A problem with Claude Code itself** (`problem`: `{kind, message,
 * resetsAt?}`) in plain words, and its one button: `{line, check, action}`
 * — `action` 'signIn' (Claude Code's own sign-in, in a terminal) or null;
 * `check` what to look at.
 */
export function problemWords(problem) {
  if (!problem?.kind) return null;
  const line = problem.message || '';
  switch (problem.kind) {
    case 'notSignedIn':
      return { line: line || "Claude Code isn't signed in.", check: 'Sign in to Claude Code, then press Continue.', action: 'signIn' };
    case 'rateLimited':
      return {
        line: line || "You've reached your Claude plan's limit.",
        check: problem.resetsAt ? `It resets at ${clock(problem.resetsAt)}. Press Continue then.` : 'It resets in a few hours. Press Continue then.',
        action: null,
      };
    case 'billing':
      return { line: line || "Claude couldn't bill this account.", check: 'Check your billing in your Claude account settings, then press Continue.', action: null };
    case 'account':
      return { line: line || "This Claude account can't be used right now.", check: 'Check the account Claude Code is signed in with (claude auth status in a terminal).', action: null };
    case 'paysAnotherWay':
      return {
        line: line || 'Claude Code is set up to pay a different way than Yap asked.',
        check: `Check Claude Code's own settings for an API key, or choose how agents pay in Settings → ${FEATURE_NAME}.`,
        action: null,
      };
    default:
      return { line, check: '', action: null };
  }
}

/** Claude Code's modes as the Mode chip says them (agents/modes.rs `label`). */
export const MODE_LABELS = {
  auto: 'Auto',
  manual: 'Manual',
  acceptEdits: 'Accept edits',
  plan: 'Plan',
  bypassPermissions: 'Bypass permissions',
};

export const modeLabel = (mode) => MODE_LABELS[mode] || (mode ? String(mode) : 'Auto');

/**
 * What Bypass permissions means, said wherever it's chosen (its one-time
 * OK, the Mode chip's tooltip, a workspace that lets it run): Claude Code
 * asks nothing, and nor does Yap before it sends an agent
 * (chat/tools/agents.rs `ask_first`: no card in Bypass, after outside
 * content either).
 */
export const BYPASS_WARNING =
  'Claude Code can run anything on your PC without asking. Yap won’t ask before it sends an agent either, even after the chat read the web or looked at your screen. Use it only in folders you trust.';

/** A Claude model id as people say it: "claude-sonnet-5-5" → "Claude Sonnet 5.5". */
export function modelLabel(id) {
  const s = String(id || '').trim();
  const m = /^claude-([a-z]+)-(\d+)(?:-(\d+))?(.*)$/i.exec(s);
  if (!m) return s;
  const family = m[1].charAt(0).toUpperCase() + m[1].slice(1).toLowerCase();
  return `Claude ${family} ${m[2]}${m[3] ? `.${m[3]}` : ''}${m[4] || ''}`;
}

/** Yap's own tool in an agent's session (its permission prompts': `mcp__yap__approve`): never a step to tell. */
const OWN_TOOL = /^Using yap · approve$/;

/**
 * What kind of step a line is (its icon): 'read', 'edit', 'command',
 * 'web', 'search', 'tool' (another tool's call: an MCP server's or a
 * connector's "Using github · create issue", a skill, a tool Yap has no
 * words for), 'helper' (part of it handed to a subagent), 'ask' (waiting
 * for you), 'other' (housekeeping: "Starting Claude Code", a note read, a
 * denial, Claude Code looking a tool up or checking on a command).
 */
export function stepKind(s) {
  if (s?.command) return 'command';
  if (s?.file) return 'edit';
  const t = String(s?.step || '');
  if (/^(Editing|Writing)\b/.test(t)) return 'edit';
  if (/^Running\b/.test(t)) return 'command';
  if (/^Searching the web\b/.test(t) || /^Reading [a-z0-9-]+(\.[a-z0-9-]+)+\//i.test(t)) return 'web';
  if (/^(Reading|Looking)\b/.test(t)) return 'read';
  if (/^Searching the files\b/.test(t)) return 'search';
  if (/^Handing part of it to a helper\b/.test(t)) return 'helper';
  if (/^Using\b/.test(t) && !OWN_TOOL.test(t)) return 'tool';
  if (/^Waiting for you\b/.test(t)) return 'ask';
  return 'other';
}

/** Whose tool a 'tool' step used, as its line says it: "Using github · create issue" → "github", "Using a skill · tidy" → "a skill", "Using Frobnicate" → "Frobnicate". */
const toolOwner = (s) => String(s?.step || '').replace(/^Using\s+/, '').split(' · ')[0].trim();

/** The Mode chip's menu: Claude Code's own keys, 1 to 5, in its order. */
export const MODE_KEYS = ['1', '2', '3', '4', '5'];

/** The mode a key picks from `modes` (the menu's order), or null. */
export function modeForKey(modes, key) {
  const i = MODE_KEYS.indexOf(String(key));
  return i >= 0 && modes?.[i] ? modes[i].mode : null;
}

/** "1 agent working", "2 agents working". */
export const runningLine = (n) => (n === 1 ? '1 agent working' : `${n} agents working`);
