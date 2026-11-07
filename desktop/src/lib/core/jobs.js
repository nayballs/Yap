// Jobs (desktop/src-tauri/src/jobs.rs): Chat or the Voice Assistant hands a
// task to the person's own Claude Code, in a folder they chose. These are
// the words the Jobs screens share — a job's status, what it costs and how
// it's paid, how long it has run, its steps from its log, the pills under
// the answer that started it, a problem in plain words with its one button
// — and the Mode chip's keys. No Svelte state: pure, tested by
// desktop/e2e/jobs-ui.spec.js.

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

/** Dollars, as small as a job's cost can be: "$0.04", "$0.004", "< $0.001". */
export function money(usd) {
  const c = Number(usd);
  if (!(c > 0)) return '$0.00';
  if (c >= 0.01) return `$${c.toFixed(2)}`;
  if (c >= 0.001) return `$${c.toFixed(3)}`;
  return '< $0.001';
}

/**
 * **How a job is paid, and what it cost**, told apart: on the person's
 * Claude plan the cost is only what the API would have charged ("Your
 * Claude plan · ≈ $0.04 on the API"; Claude Code's estimate); on their API
 * key it's what it cost ("API key · $0.04").
 */
export function costLine(job) {
  const cost = Number(job?.cost) || 0;
  if (job?.pay === 'key') return `API key · ${money(cost)}`;
  return cost > 0 ? `Your Claude plan · ≈ ${money(cost)} on the API` : 'Your Claude plan';
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
 * once in a row, with the file it changes or the command it runs.
 * `[{step, file, command, ts}]`.
 */
export function stepsOf(log) {
  const out = [];
  for (const e of log || []) {
    const step = typeof e?.step === 'string' ? e.step.trim() : '';
    if (!step) continue;
    const last = out[out.length - 1];
    if (last && last.step === step && !e.file && !e.command) continue;
    out.push({ step, file: e.file || null, command: e.command || null, ts: Number(e.ts) || 0 });
  }
  return out;
}

/** The step a job is on now, from its log: its latest line ('' once it's over or before any). */
export function latestStep(log, status) {
  if (!isLive(status)) return '';
  const steps = stepsOf(log);
  return steps.length ? steps[steps.length - 1].step : '';
}

/** The message index a chat message id names ("chat:12:4" → 4), or -1. */
export function messageIndex(id, conversation) {
  const m = /^chat:(\d+):(\d+)$/.exec(String(id || ''));
  if (!m) return -1;
  if (conversation != null && Number(m[1]) !== Number(conversation)) return -1;
  return Number(m[2]);
}

/**
 * **The job pills under each answer**: the jobs of `conversation`, each
 * under the first answer after the question that started it (`messageId`).
 * `messages`: the thread's (`role`). A Map from the answer's index to its
 * jobs, oldest first.
 */
export function pillsByAnswer(jobs, conversation, messages) {
  const out = new Map();
  if (conversation == null || !messages?.length) return out;
  const mine = (jobs || []).filter((j) => j?.conversation === conversation).sort((a, b) => (a.createdTs || 0) - (b.createdTs || 0));
  for (const j of mine) {
    const q = messageIndex(j.messageId, conversation);
    if (q < 0) continue;
    let at = -1;
    for (let i = q + 1; i < messages.length; i++) {
      if (messages[i]?.kind === 'jobReport') continue;
      if (messages[i]?.role === 'assistant') {
        at = i;
        break;
      }
      if (messages[i]?.role === 'user') break;
    }
    if (at < 0) continue;
    out.set(at, [...(out.get(at) || []), j]);
  }
  return out;
}

/**
 * The job a report marker message is about: the one whose `reportMessageId`
 * it is (a job keeps its latest report's), else — an earlier turn's report
 * of a job reported again since — the newest job in that chat and folder.
 * null when its job is gone (the card still says what the marker says).
 */
export function jobOfReport(jobs, markerId, conversation, folder = '') {
  const list = jobs || [];
  const exact = list.find((j) => j?.reportMessageId && j.reportMessageId === markerId);
  if (exact) return exact;
  if (!folder) return null;
  return list.filter((j) => j?.conversation === conversation && j.folder === folder).sort((a, b) => (b.createdTs || 0) - (a.createdTs || 0))[0] ?? null;
}

/** The report card's line: "Job done", "Job's plan is ready", "Job has a question", "Job didn't finish". */
export function reportTitle(job, markerText = '') {
  // (A job at work again since says what the marker said.)
  if (job && !isLive(job.status)) {
    if (job.status === 'failed') return "Job didn't finish";
    if (job.plan) return "Job's plan is ready";
    if (job.asks) return 'Job has a question';
    if (job.status === 'stopped') return 'Job stopped';
    return 'Job done';
  }
  // From the marker's own words (jobs/report.rs `marker_text`).
  if (/: it didn't finish\./.test(markerText)) return "Job didn't finish";
  if (/: it made a plan/.test(markerText)) return "Job's plan is ready";
  if (/asks the user something/.test(markerText)) return 'Job has a question';
  return 'Job done';
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
