// What an answer's tools did, and what a turn is doing now (chat/activity.rs).
//
// - `chipKind(chip)`: which mark a tool's chip gets (the web, the calendar, a
//   document, the safety rule, any other tool).
// - `stepsSummary(chips)`: the chips folded into one line under the answer,
//   as the Claude Code app folds its steps ("Ran a command, used Claude Docs
//   2 times"): "Searched the web 3 times, read 2 pages, wrote 4 sections".
//   The chips themselves are the list it opens to (ChatSteps.svelte).
// - `elapsed(s)`: a turn's time on the line beside its step ("12s", "1m 59s").

// A chip about the web (web.rs) or the safety rule (chat/tools.rs).
const WEB_CHIP = /^(Searched the web|Couldn't search the web|Didn't search for|Read [^"]|Couldn't read [^"]|Didn't read |Answered from )/;
// The calendar's (chat/tools/calendar.rs).
const CALENDAR_CHIP = /^(Checked your calendar|Found \d+ meetings? with|No meetings with|Free |Busy |No calendar connected|Couldn't read the (day|time) )/;
// A document's (chat/documents.rs, and the rich tools': documents/rich.rs).
const DOC_CHIP =
  /^(Started "|(Re)?[Ww]rote (\d+ sections|"[^"]*") in "|Added "[^"]*" to "|Didn't (re)?write "|"[^"]*" is already written in "|Couldn't (start a document|rewrite a section)|Read (the document|what changed in) "|Changed (a table in|a part of) "|Added to "|Took a part out of "|Moved a part of "|Made \d+ changes in "|Didn't (change|restore) "|Couldn't (change|restore) ("|a document)|Looked at the history of "|Restored version \d+ of "|"[^"]*" is already as version )/;

/** A chip's mark: 'guard' | 'web' | 'calendar' | 'doc' | 'tool'. */
export const chipKind = (t) =>
  /: not after reading the web$/.test(t)
    ? 'guard'
    : WEB_CHIP.test(t)
      ? 'web'
      : CALENDAR_CHIP.test(t)
        ? 'calendar'
        : DOC_CHIP.test(t)
          ? 'doc'
          : 'tool';

const n1 = (n, one, many) => (n === 1 ? one : many.replace('{n}', String(n)));
const times = (n, what) => (n === 1 ? what : `${what} ${n} times`);

// What each kind of step says, folded: [test, key, how many it counts for].
// The first that matches wins; a step that didn't go through ("Couldn't…",
// "Didn't…", the safety rule) is counted apart.
const STEPS = [
  [/^(Couldn't|Didn't) |: not after reading the web$/, 'failed'],
  [/^Searched the web/, 'web'],
  [/^Answered from (\d+) web pages?/, 'pages', (m) => Number(m[1])],
  [/^(Read (the document|what changed in) "|Looked at the history of ")/, 'readDoc'],
  [/^Read "/, 'readNote'],
  [/^Read /, 'pages'],
  [/^Started "/, 'doc'],
  [/^Rewrote (?:(\d+) sections|"[^"]*") in "/, 'rewrote', (m) => (m[1] ? Number(m[1]) : 1)],
  [/^[Ww]rote (?:(\d+) sections|"[^"]*") in "/, 'wrote', (m) => (m[1] ? Number(m[1]) : 1)],
  [/^(Added "[^"]*" to "|Added to "|Changed (a table in|a part of) "|Took a part out of "|Moved a part of "|Made \d+ changes in "|Restored version \d+ of ")/, 'docEdit'],
  [/^(Found \d+ notes? for|No notes found for|No notes mention)/, 'notes'],
  [/^Searched your meetings/, 'meetings'],
  [/^Ran \//, 'skill'],
  [/^(Edited |Wrote in |Wrote a summary for )/, 'noteEdit'],
  [/^Created note/, 'note'],
  [CALENDAR_CHIP, 'calendar'],
  [/^(Remembered|Forgot)/, 'memory'],
  [/^(Found \d+ past chats? for|No past chats)/, 'chats'],
  [/^Opened /, 'opened'],
  [/^(Found \d+ apps? for|Listed |No apps? )/, 'apps'],
  [/^Copied/, 'copied'],
];

const SAY = {
  web: (n) => times(n, 'searched the web'),
  pages: (n) => n1(n, 'read a page', 'read {n} pages'),
  readDoc: (n) => times(n, 'read the document'),
  readNote: (n) => n1(n, 'read a note', 'read {n} notes'),
  doc: (n) => n1(n, 'started a document', 'started {n} documents'),
  wrote: (n) => n1(n, 'wrote a section', 'wrote {n} sections'),
  rewrote: (n) => n1(n, 'rewrote a section', 'rewrote {n} sections'),
  docEdit: (n) => times(n, 'changed the document'),
  notes: (n) => times(n, 'searched your notes'),
  meetings: (n) => times(n, 'searched your meetings'),
  skill: (n) => n1(n, 'ran a skill', 'ran {n} skills'),
  noteEdit: (n) => n1(n, 'changed a note', 'changed notes {n} times'),
  note: (n) => n1(n, 'made a note', 'made {n} notes'),
  calendar: (n) => times(n, 'checked your calendar'),
  memory: () => "updated Yap's memory",
  chats: (n) => times(n, 'searched your chats'),
  opened: (n) => n1(n, 'opened an app', 'opened {n} apps'),
  apps: () => 'looked for an app',
  copied: () => 'copied to the clipboard',
  tool: (n) => n1(n, 'used a tool', 'used {n} tools'),
  failed: (n) => n1(n, "1 didn't go through", "{n} didn't go through"),
};

/**
 * An answer's chips as one line, in the order they first came: "Searched
 * the web 3 times, read 2 pages, wrote 4 sections". Pure.
 */
export function stepsSummary(chips) {
  const counts = new Map();
  for (const chip of chips ?? []) {
    const t = String(chip).trim();
    if (!t) continue;
    let key = 'tool';
    let n = 1;
    for (const [test, k, count] of STEPS) {
      const m = t.match(test);
      if (m) {
        key = k;
        n = count ? count(m) : 1;
        break;
      }
    }
    counts.set(key, (counts.get(key) ?? 0) + n);
  }
  // What didn't go through says so last.
  const failed = counts.get('failed');
  counts.delete('failed');
  if (failed) counts.set('failed', failed);
  const line = [...counts].map(([k, n]) => SAY[k](n)).join(', ');
  return line ? line[0].toUpperCase() + line.slice(1) : '';
}

/** A turn's time so far: "12s", "1m 59s", "1h 4m". Pure. */
export function elapsed(seconds) {
  const s = Math.max(0, Math.floor(seconds || 0));
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m ${s % 60}s`;
  return `${Math.floor(s / 3600)}h ${Math.floor((s % 3600) / 60)}m`;
}
