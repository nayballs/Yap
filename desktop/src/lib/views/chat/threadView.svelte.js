// **How a Code chat's thread shows Claude's work** (15 Nov 2026, as Claude
// Code's app has its transcript view): Normal — each stretch of steps one
// folded line, the default —, Thinking — Claude's thinking too, in order,
// between the stretches it came between —, or Verbose — that, and every
// stretch and row open. Chosen from the chat's ⋯ in the list (Transcript
// view), kept per chat in this window's storage (a convenience: nothing of
// it is the chat's data, and a chat with none is Normal). AgentWork reads it.

const KEY = 'yapThreadViews';

/** The views, in the menu's order. */
export const THREAD_VIEWS = [
  { id: 'normal', label: 'Normal' },
  { id: 'thinking', label: 'Thinking' },
  { id: 'verbose', label: 'Verbose' },
];
const KNOWN = new Set(THREAD_VIEWS.map((v) => v.id));

function read() {
  try {
    const v = JSON.parse(localStorage.getItem(KEY) || '{}');
    return v && typeof v === 'object' ? v : {};
  } catch {
    return {};
  }
}

/** Chat id → its view, when it isn't Normal. */
const views = $state({ by: read() });

/** Chat `id`'s view ('normal' when none was chosen). */
export function threadViewOf(id) {
  const v = id == null ? null : views.by[String(id)];
  return KNOWN.has(v) ? v : 'normal';
}

/** Show chat `id`'s thread as `view` (kept for it). */
export function setThreadView(id, view) {
  if (id == null || !KNOWN.has(view)) return;
  const next = { ...views.by };
  if (view === 'normal') delete next[String(id)];
  else next[String(id)] = view;
  views.by = next;
  try {
    if (Object.keys(next).length) localStorage.setItem(KEY, JSON.stringify(next));
    else localStorage.removeItem(KEY);
  } catch {
    /* private storage: just this session */
  }
}

/** Whether `view` shows Claude's thinking (Thinking and Verbose). */
export const showsThinking = (view) => view === 'thinking' || view === 'verbose';
