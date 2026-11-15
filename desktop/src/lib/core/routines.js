// Routines in words (desktop/src-tauri/src/chat/routines.rs): Skills that
// run by themselves — on a schedule, after a meeting, or before one on the
// calendar — on this PC while Yap runs. Pure: Settings → Skills → Routines
// (settings/RoutinesSection.svelte) draws what comes out, and the e2e spec
// checks it directly (routines.spec.js).

/** A schedule's days. */
export const DAYS = [
  { value: 'daily', label: 'Every day' },
  { value: 'weekdays', label: 'Weekdays' },
  { value: 'weekly', label: 'Once a week' },
];

/** Monday first (Rust's 0…6). */
export const WEEKDAYS = ['Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday', 'Sunday'];

/** When it runs. */
export const TRIGGERS = [
  { value: 'schedule', label: 'On a schedule' },
  { value: 'meetingEnd', label: 'After a meeting' },
  { value: 'beforeEvent', label: 'Before a meeting' },
];

/** How long before a calendar meeting (minutes; Rust allows 1–60). */
export const LEADS = [5, 10, 15, 30, 60];

/** Where a meeting's run goes. */
export const DESTS = [
  { value: 'chat', label: 'Its own chat' },
  { value: 'note', label: "With the meeting's note" },
];

/** Runs a day at most, every routine together (Rust's `DAILY_CAP`). */
export const DAILY_CAP = 20;
/** A missed time still runs up to this many hours late (Rust's `GRACE_SECS`). */
export const GRACE_HOURS = 3;

/** "09:00"-style times every `step` minutes, for the time picker. */
export function times(step = 15) {
  const out = [];
  for (let m = 0; m < 24 * 60; m += step) {
    const t = `${String(Math.floor(m / 60)).padStart(2, '0')}:${String(m % 60).padStart(2, '0')}`;
    out.push({ value: t, label: t });
  }
  return out;
}

/** "9:05" → "09:05"; anything else null. */
export function cleanTime(s) {
  const m = /^\s*(\d{1,2}):(\d{2})\s*$/.exec(String(s ?? ''));
  if (!m) return null;
  const h = Number(m[1]);
  const min = Number(m[2]);
  if (h > 23 || min > 59) return null;
  return `${String(h).padStart(2, '0')}:${m[2]}`;
}

/** When a routine runs, in a line: "Weekdays at 09:00", "After meetings in Clients", "10 min before each meeting". */
export function triggerLine(t) {
  if (!t) return '';
  if (t.kind === 'meetingEnd') {
    const folder = (t.folder || '').trim();
    if (folder && t.outside) return `After meetings in ${folder} with people from outside`;
    if (folder) return `After meetings in ${folder}`;
    if (t.outside) return 'After meetings with people from outside';
    return 'After each meeting';
  }
  if (t.kind === 'beforeEvent') return `${t.minutes ?? 10} min before each meeting`;
  const time = cleanTime(t.time) || t.time || '';
  if (t.days === 'weekdays') return `Weekdays at ${time}`;
  if (t.days === 'weekly') return `${WEEKDAYS[Math.min(6, Math.max(0, t.weekday ?? 0))]}s at ${time}`;
  return `Every day at ${time}`;
}

/** Where a run's answer goes. */
export function whereLine(r) {
  return r?.to === 'note' ? "With the meeting's note" : 'In its own chat';
}

/** A meeting's routine may go with the meeting's note: its Skill runs on one meeting. */
export function canGoWithNote(trigger, skill) {
  return trigger?.kind === 'meetingEnd' && !!skill && skill.runsOn !== 'meetings';
}

const pad = (n) => String(n).padStart(2, '0');
const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
const SHORT_DAYS = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];

/** A moment (unix seconds) as the lists say it: "Today, 09:00", "Yesterday, 16:00", "Tomorrow, 09:00", "Mon 17 Nov, 09:00". */
export function when(ts, now = Date.now() / 1000) {
  if (!ts) return '';
  const d = new Date(ts * 1000);
  const n = new Date(now * 1000);
  const day = (x) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const diff = Math.round((day(d) - day(n)) / 86_400_000);
  const time = `${pad(d.getHours())}:${pad(d.getMinutes())}`;
  if (diff === 0) return `Today, ${time}`;
  if (diff === -1) return `Yesterday, ${time}`;
  if (diff === 1) return `Tomorrow, ${time}`;
  return `${SHORT_DAYS[d.getDay()]} ${d.getDate()} ${MONTHS[d.getMonth()]}, ${time}`;
}

/** "HH:MM" of a moment. */
function clock(ts) {
  const d = new Date(ts * 1000);
  return `${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** A routine's own line under its name: when it runs, where it goes, and when next. */
export function routineLine(r, now = Date.now() / 1000) {
  const parts = [triggerLine(r.trigger), whereLine(r)];
  if (r.skillGone) return 'Its Skill was deleted: edit it to pick another.';
  if (!r.on) parts.push('Off');
  else if (r.next) parts.push(`Next: ${when(r.next, now).replace(/^(Today|Tomorrow|Yesterday)/, (w) => w.toLowerCase())}`);
  else if (r.trigger?.kind === 'beforeEvent') parts.push('No meeting coming up');
  return parts.filter(Boolean).join(' · ');
}

/** A run's state as its pill says it. `tone`: ok | bad | quiet | busy. */
export function runState(run) {
  switch (run?.state) {
    case 'done':
      return { label: run.cause === 'missed' ? 'Ran late' : 'Ran', tone: 'ok' };
    case 'failed':
      return { label: 'Failed', tone: 'bad' };
    case 'skipped':
      return { label: 'Skipped', tone: 'quiet' };
    case 'running':
      return { label: 'Running…', tone: 'busy' };
    default:
      return { label: 'Waiting', tone: 'busy' };
  }
}

/** What started a run: "On its schedule", "Run now", "After "Supplier call"", "Before "Weekly sync"", "Missed 09:00". */
export function causeLine(run) {
  switch (run?.cause) {
    case 'now':
      return 'Run now';
    case 'meeting':
      return run.about ? `After “${run.about}”` : 'After a meeting';
    case 'event':
      return run.about ? `Before “${run.about}”` : 'Before a meeting';
    case 'missed':
      return run.slot ? `Missed ${clock(run.slot)}` : 'Missed its time';
    default:
      return 'On its schedule';
  }
}

/** The line under a run: why it failed, was skipped or waits; a late run's reason. */
export function runWhy(run) {
  if (run?.why) return run.why;
  if (run?.state === 'done' && run.cause === 'missed' && run.slot)
    return `Yap wasn't running at ${clock(run.slot)}, so it ran when Yap found it.`;
  return '';
}

/** A run's result to open: { chat } | { note } | null. */
export function runResult(run) {
  if (run?.state !== 'done') return null;
  if (run.chat) return { chat: run.chat };
  if (run.note) return { note: run.note };
  return null;
}

/** Today's runs against the cap, in words. */
export function capLine(today, cap = DAILY_CAP) {
  if (today >= cap) return `Today's ${cap} runs are done: routines run again tomorrow.`;
  return `${today} of ${cap} runs today.`;
}

/** A new routine's form, for `skill` (or none picked yet). */
export function blankRoutine(skill = null) {
  return {
    id: '',
    skill: skill?.id ?? '',
    words: '',
    trigger: { kind: 'schedule', days: 'weekdays', weekday: 0, time: '09:00' },
    on: true,
    to: 'chat',
  };
}

/** The trigger for kind `kind`, keeping what `was` had of it. */
export function triggerOf(kind, was = {}) {
  if (kind === 'meetingEnd') return { kind, folder: was.folder ?? '', outside: !!was.outside };
  if (kind === 'beforeEvent') return { kind, minutes: was.minutes ?? 10 };
  return { kind: 'schedule', days: was.days ?? 'weekdays', weekday: was.weekday ?? 0, time: was.time ?? '09:00' };
}

/** What's wrong with a routine's form (`skills`: every Skill), or ''. */
export function routineProblem(draft, skills) {
  if (!draft) return '';
  if (!draft.skill || !skills.some((s) => s.id === draft.skill)) return 'Pick a Skill for it to run.';
  const t = draft.trigger;
  if (t?.kind === 'schedule' && !cleanTime(t.time)) return 'Give it a time of day, like 09:00.';
  if (t?.kind === 'beforeEvent' && !(t.minutes >= 1 && t.minutes <= 60)) return 'It runs between 1 and 60 minutes before a meeting.';
  if ((draft.words || '').length > 200) return 'Its words can be 200 characters at most.';
  return '';
}

/** The Skills a routine may run, for its picker: yours first, then Yap's, each "/command · Name". */
export function skillOptions(skills) {
  const mine = skills.filter((s) => !s.builtin);
  const yaps = skills.filter((s) => s.builtin);
  return [
    ...mine.map((s) => ({ value: s.id, label: `/${s.command} · ${s.name}`, group: 'Your Skills' })),
    ...yaps.map((s) => ({ value: s.id, label: `/${s.command} · ${s.name}`, group: "Yap's Skills" })),
  ];
}

/** The form's routine as Rust takes it (a meeting's "with the note" only where it can go). */
export function toSave(draft, skills) {
  const skill = skills.find((s) => s.id === draft.skill);
  const trigger = triggerOf(draft.trigger.kind, draft.trigger);
  if (trigger.kind === 'schedule') trigger.time = cleanTime(trigger.time) || trigger.time;
  return {
    id: draft.id || '',
    skill: draft.skill,
    words: (draft.words || '').trim(),
    trigger,
    on: draft.on !== false,
    to: draft.to === 'note' && canGoWithNote(trigger, skill) ? 'note' : 'chat',
  };
}
