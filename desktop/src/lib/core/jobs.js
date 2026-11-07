// Jobs (desktop/src-tauri/src/jobs.rs): Chat or the Voice Assistant hands a
// task to the person's own Claude Code, in a folder they chose. These are
// the words the Jobs screens share — a job's status, what it costs and how
// it's paid, how long it has run, its timeline from its log (steps and what
// it said between them), the line it's on now, Claude Code's to-do list,
// what changed in each file and the commands it ran, the pills under the
// answer that started it, a problem in plain words with its one button —
// and the Mode chip's keys. No Svelte state: pure, tested by
// desktop/e2e/jobs-ui.spec.js.
import { dollars } from './money.js';

/** A job's status as the screens say it. */
export const STATUS_WORDS = {
  queued: 'Waiting to start',
  running: 'Running',
  asking: 'Waiting for you',
  done: 'Done',
  failed: "Didn't finish",
  stopped: 'Stopped',
  interrupted: 'Interrupted',
};

/** Not over yet: queued, running, or waiting for the person to allow a step. */
export const isLive = (status) => status === 'queued' || status === 'running' || status === 'asking';

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
 * **Undo job is offered**: it's over, it changed files, and their backups
 * are there — not undone already, not swept (7 days on), and made at all
 * (`noBackups`: Claude Code's hooks didn't run, so nothing was backed up).
 */
export const canUndo = (job) => !!job && !isLive(job.status) && (job.files?.length ?? 0) > 0 && !job.undone && !job.swept && !job.noBackups;

/** The job's tab when it changed files with no backups made. */
export const NO_BACKUPS = "No backups were made, so Undo can't put these files back.";

/** Dollars as an answer's byline says them (core/money.js): "$0.04", "$0.0042", "under $0.0001". */
export const money = (usd) => dollars((Number(usd) || 0) * 1_000_000);

/**
 * **How a job is paid, and what it cost**, told apart — in the words of a
 * plan chat's answers (core/aiModel.js `paidLine`): on the person's Claude
 * plan the cost is only what the API would have charged ("Your Claude plan
 * · ≈ $0.0042 on the API"; Claude Code's estimate); on their API key it's
 * what it cost ("API key · $0.0042"). Nothing yet: the way alone.
 */
export function costLine(job) {
  const cost = Number(job?.cost) || 0;
  const way = job?.pay === 'key' ? 'API key' : 'Your Claude plan';
  if (!(cost > 0)) return way;
  return job?.pay === 'key' ? `API key · ${money(cost)}` : `Your Claude plan · ≈ ${money(cost)} on the API`;
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
 * How long a job has run (`now`: unix seconds): from its first start to
 * now while it isn't over, else to its latest turn's end. '' before it ever
 * started.
 */
export function elapsed(job, now = Date.now() / 1000) {
  const from = Number(job?.startedTs) || 0;
  if (!from) return '';
  const to = isLive(job.status) ? now : Number(job.endedTs) || Number(job.updatedTs) || now;
  return duration(to - from);
}

/**
 * A job's steps from its log (`yap-job` events, oldest first): each line
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

/**
 * **A job's timeline** from its log, oldest first: its steps (each line
 * once in a row) and what it said between them, in order —
 * `{kind: 'step', step, file, command, about, ts}` or `{kind: 'said', text,
 * ts}`. `final`: its final words, shown apart, so the same words at the
 * end aren't said twice.
 */
export function timelineOf(log, final = '') {
  const out = [];
  for (const e of log || []) {
    const said = saidOf(e);
    if (said) {
      out.push({ kind: 'said', text: said, ts: Number(e.ts) || 0 });
      continue;
    }
    const step = typeof e?.step === 'string' ? e.step.trim() : '';
    if (!step) continue;
    const last = out[out.length - 1];
    if (last?.kind === 'step' && last.step === step && !e.file && !e.command) continue;
    out.push({ kind: 'step', step, file: e.file || null, command: e.command || null, about: e.about || null, ts: Number(e.ts) || 0 });
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

/** Narration long enough to fold (its first lines shown, "Show more"). */
export const longSaid = (text) => String(text || '').length > 280 || String(text || '').split('\n').length > 4;

/** The line of a job whose plan waits for the person's Go ahead (jobs/runner.rs `on_tick`). */
export const PLAN_WAITING = 'Waiting for you: its plan is ready';

/**
 * **The line a job is on now**, from its log: its latest step, or the line
 * it moved to without one (`now`: the to-do in progress once a call came
 * back, the line it was on before it asked). '' once it's over or before
 * any.
 */
export function latestStep(log, status) {
  if (!isLive(status)) return '';
  // Waiting on the person's Allow / Deny: that, whatever line came after it
  // (a to-do's update can land a moment after the wait began) — a plan's
  // Go ahead / Not yet as its own line (jobs/runner.rs).
  if (status === 'asking') {
    const asked = [...(log || [])].reverse().find((e) => typeof e?.step === 'string' && /^Waiting for you\b/.test(e.step));
    return asked?.step === PLAN_WAITING ? PLAN_WAITING : 'Waiting for you to allow a step';
  }
  let line = '';
  for (const e of log || []) {
    if (typeof e?.now === 'string' && e.now.trim()) line = e.now.trim();
    else if (typeof e?.step === 'string' && e.step.trim()) line = e.step.trim();
  }
  return line;
}

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

/** **What changed in each file**, from the log's diffs (`diff.rs`): a Map from the file (as the job's files say it) to its diffs, in order. */
export function diffsOf(log) {
  const out = new Map();
  for (const e of log || []) {
    const d = e?.diff;
    if (!d?.file || !Array.isArray(d.lines)) continue;
    out.set(d.file, [...(out.get(d.file) || []), d]);
  }
  return out;
}

/** A file's diffs added up: `{added, removed, made}` — `made` "new" when the job made it (its first write a new file). */
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

/** A diff line's mark and text: `{mark: 'del' | 'add' | 'ctx' | 'gap', text}`. */
export function diffLine(l) {
  const s = String(l ?? '');
  if (s === '@') return { mark: 'gap', text: '' };
  const m = s.charAt(0);
  return { mark: m === '-' ? 'del' : m === '+' ? 'add' : 'ctx', text: s.slice(1) };
}

/**
 * **The commands it ran**, each with what Claude Code said it's for:
 * `[{command, about}]` — the job's own list (`job.commands`), with the
 * descriptions its log's events carry, in order.
 */
export function commandsOf(job, log) {
  const told = (log || []).filter((e) => typeof e?.command === 'string');
  return (job?.commands || []).map((command, i) => ({ command, about: told[i]?.command === command ? told[i].about || '' : '' }));
}

/** The message index a chat message id names ("chat:12:4" → 4), or -1. */
export function messageIndex(id, conversation) {
  const m = /^chat:(\d+):(\d+)$/.exec(String(id || ''));
  if (!m) return -1;
  if (conversation != null && Number(m[1]) !== Number(conversation)) return -1;
  return Number(m[2]);
}

/**
 * **A request in a few words** (a turn's header; jobs/store.rs `title_of`):
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
 * **A job's turns** (jobs/store.rs `TurnNote`: 2026-11-07, one job per Code
 * chat): each request to it and what came of it — `{n, title, request,
 * status, startedTs, endedTs, cost, files, denials, text, plan, asks, error,
 * undone, messageId, reportMessageId, events}` — its log split among them
 * (each event's `turn`; an event without one goes with the turn before it,
 * an older log's all with its first). A job from before turns: one, made
 * from the job. An event naming a turn this window hasn't read yet gets a
 * turn of its own until it does.
 */
export function turnsOf(job, log) {
  if (!job) return [];
  const notes = job.turnList?.length
    ? job.turnList
    : [
        {
          n: 1,
          title: titleOf(job.brief),
          request: job.brief || '',
          status: job.status,
          startedTs: job.startedTs,
          endedTs: job.endedTs,
          cost: job.cost,
          files: job.files || [],
          denials: job.denials || [],
          text: job.text || '',
          plan: job.plan,
          asks: job.asks,
          error: job.error,
          undone: job.undone,
          messageId: job.messageId,
          reportMessageId: job.reportMessageId,
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
 * **How long a job has worked**, its turns added up (the time between them
 * left out: a follow-up asked hours later isn't hours of work); a job from
 * before turns, `elapsed`. '' before it ever started.
 */
export function workedFor(job, now = Date.now() / 1000) {
  const turns = (job?.turnList || []).filter((t) => Number(t.startedTs) > 0);
  if (turns.length < 2) return elapsed(job, now);
  const last = turns.at(-1);
  const secs = turns.reduce((sum, t) => {
    const to = t === last && isLive(job.status) ? now : Number(t.endedTs) || Number(t.startedTs);
    return sum + Math.max(0, to - Number(t.startedTs));
  }, 0);
  return duration(secs);
}

/** The turn a job is on now (the newest that ran), for how long it has been at it: the running list's time. */
export const turnNow = (job) => [...(job?.turnList || [])].reverse().find((t) => Number(t.startedTs) > 0) ?? null;

/** A turn's status as its header says it: queued behind the one working, never sent (stopped first), else the job's words. */
export function turnStatusWord(t) {
  if (t?.status === 'queued' && !t.startedTs) return 'Queued';
  if ((t?.status === 'stopped' || t?.status === 'interrupted') && !t.startedTs) return 'Not sent yet';
  return statusWord(t?.status);
}

/** **Undo this turn** is offered: the job is over, the turn changed files, and their backups are there (as `canUndo`). */
export const canUndoTurn = (job, turn) => !!job && !!turn && !isLive(job.status) && (turn.files?.length ?? 0) > 0 && !turn.undone && !job.swept && !job.noBackups;

/** The answer after message `q`: the first assistant message before the person's next (a report marker skipped), or -1. */
function answerAfter(messages, q) {
  for (let i = q + 1; i < messages.length; i++) {
    if (messages[i]?.kind === 'jobReport') continue;
    if (messages[i]?.role === 'assistant') return i;
    if (messages[i]?.role === 'user') return -1;
  }
  return -1;
}

/**
 * **The job pills under each answer**: each turn of the jobs of
 * `conversation` under the first answer after the question that asked for
 * it (its `messageId`; a job from before turns: the job's), one pill a job
 * an answer (its newest turn there) — `{...job, turn, turnStatus,
 * turnStarted, turnTitle}`. `messages`: the thread's (`role`). A Map from the answer's
 * index to its pills, oldest first.
 */
export function pillsByAnswer(jobs, conversation, messages) {
  const out = new Map();
  if (conversation == null || !messages?.length) return out;
  const mine = (jobs || []).filter((j) => j?.conversation === conversation).sort((a, b) => (a.createdTs || 0) - (b.createdTs || 0));
  for (const j of mine) {
    const turns = j.turnList?.length ? j.turnList : [{ n: 1, messageId: j.messageId, status: j.status, title: '' }];
    for (const t of turns) {
      const q = messageIndex(t.messageId, conversation);
      if (q < 0) continue;
      const at = answerAfter(messages, q);
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
 * The job a report marker message is about: the one whose `reportMessageId`
 * it is, or one of whose turns' it is (a job keeps its latest report's, each
 * turn its own), else — an earlier report from before turns were kept — the
 * newest job in that chat and folder. null when its job is gone (the card
 * still says what the marker says).
 */
export function jobOfReport(jobs, markerId, conversation, folder = '') {
  const list = jobs || [];
  const exact = list.find((j) => j?.reportMessageId && j.reportMessageId === markerId) || list.find((j) => (j?.turnList || []).some((t) => t?.reportMessageId === markerId));
  if (exact) return exact;
  if (!folder) return null;
  return list.filter((j) => j?.conversation === conversation && j.folder === folder).sort((a, b) => (b.createdTs || 0) - (a.createdTs || 0))[0] ?? null;
}

/** The turn of `job` a report marker is about (`null`: not known, an older job's). */
export const turnOfReport = (job, markerId) => (job?.turnList || []).find((t) => t?.reportMessageId === markerId) ?? null;

/**
 * **A job tab's name** in the panel's row: "Job · garden-planner" — and,
 * when another job tab there has the same folder (a fresh job beside the
 * chat's earlier one), its first request too, so no two tabs read the
 * same ("Job · garden-planner · Add a minimap button"; failing that, "#2").
 */
export function jobTabName(t, tabs, lookup = null) {
  const folder = t?.job?.folder || 'folder';
  const same = (tabs || []).filter((o) => o?.kind === 'job' && (o.job?.folder || 'folder') === folder);
  if (same.length < 2) return `Job · ${folder}`;
  // (A tab opened from a turn's event has the job's ref only: its turns from
  // the jobs list; a job from before turns, its brief's first words.)
  const first = (o) => {
    const j = lookup?.(o.job?.id) ?? o.job;
    return j?.turnList?.[0]?.title || titleOf(j?.brief) || '';
  };
  const title = first(t);
  const named = !!title && same.filter((o) => first(o) === title).length === 1;
  return named ? `Job · ${folder} · ${title}` : `Job · ${folder} #${same.indexOf(t) + 1}`;
}

/** A plan the person allowed (ExitPlanMode's Go ahead): its report card's line. */
export const PLAN_ALLOWED = 'Job is carrying out its plan';

/** The report card's line: "Job done", "Job's plan is ready", "Job is carrying out its plan", "Job has a question", "Job didn't finish". */
export function reportTitle(job, markerText = '') {
  // (A job at work again since says what the marker said.)
  if (job && !isLive(job.status)) {
    if (job.status === 'failed') return "Job didn't finish";
    if (job.plan && job.planAllowed) return PLAN_ALLOWED;
    if (job.plan) return "Job's plan is ready";
    if (job.asks) return 'Job has a question';
    if (job.status === 'stopped') return 'Job stopped';
    return 'Job done';
  }
  // From the marker's own words (jobs/report.rs `marker_text`).
  if (/: it didn't finish\./.test(markerText)) return "Job didn't finish";
  if (/: it made a plan, and the user allowed it/.test(markerText)) return PLAN_ALLOWED;
  if (/: it made a plan/.test(markerText)) return "Job's plan is ready";
  if (/asks the user something/.test(markerText)) return 'Job has a question';
  return 'Job done';
}

/**
 * **A report's plan still waits for its go-ahead** (its card's Go ahead,
 * `job_go_ahead`): the turn it reports is a plan, not allowed already, it's
 * the job's latest turn, and the job isn't working. A job from before turns:
 * the job itself, when this is its latest report.
 */
export function planWaits(job, turn, latest = false) {
  if (!job || isLive(job.status) || !job.plan || job.planAllowed) return false;
  const last = (job.turnList || []).at(-1);
  if (turn) return !!turn.plan && !turn.planAllowed && !!last && last.n === turn.n;
  return latest && !last;
}

/** The folder a report marker names ("The job in garden-planner: …"). */
export function reportFolder(markerText) {
  const m = /^The job in (.+?): /.exec(String(markerText || ''));
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
        check: "Check Claude Code's own settings for an API key, or choose how jobs pay in Settings → Jobs.",
        action: null,
      };
    default:
      return { line, check: '', action: null };
  }
}

/** Claude Code's modes as the Mode chip says them (jobs/modes.rs `label`). */
export const MODE_LABELS = {
  auto: 'Auto',
  manual: 'Manual',
  acceptEdits: 'Accept edits',
  plan: 'Plan',
  bypassPermissions: 'Bypass permissions',
};

export const modeLabel = (mode) => MODE_LABELS[mode] || (mode ? String(mode) : 'Auto');

/** A Claude model id as people say it: "claude-sonnet-5-5" → "Claude Sonnet 5.5". */
export function modelLabel(id) {
  const s = String(id || '').trim();
  const m = /^claude-([a-z]+)-(\d+)(?:-(\d+))?(.*)$/i.exec(s);
  if (!m) return s;
  const family = m[1].charAt(0).toUpperCase() + m[1].slice(1).toLowerCase();
  return `Claude ${family} ${m[2]}${m[3] ? `.${m[3]}` : ''}${m[4] || ''}`;
}

/**
 * What kind of step a line is (its icon): 'read', 'edit', 'command',
 * 'web', 'search', 'ask' (waiting for you), 'other'.
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
  if (/^Waiting for you\b/.test(t)) return 'ask';
  return 'other';
}

/** The Mode chip's menu: Claude Code's own keys, 1 to 5, in its order. */
export const MODE_KEYS = ['1', '2', '3', '4', '5'];

/** The mode a key picks from `modes` (the menu's order), or null. */
export function modeForKey(modes, key) {
  const i = MODE_KEYS.indexOf(String(key));
  return i >= 0 && modes?.[i] ? modes[i].mode : null;
}

/** "1 job running", "2 jobs running". */
export const runningLine = (n) => (n === 1 ? '1 job running' : `${n} jobs running`);
