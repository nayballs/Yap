// What an answer's tools did, and what a turn is doing now (chat/activity.rs).
//
// - `chipKind(chip)`: which mark a tool's chip gets (the web, the calendar, a
//   document, the safety rule, any other tool).
// - `stepsSummary(chips)`: the chips folded into one line under the answer,
//   as the Claude Code app folds its steps ("Ran a command, used Claude Docs
//   2 times"): "Searched the web 3 times, read 2 pages, wrote 4 sections".
//   The chips themselves are the list it opens to (ChatSteps.svelte).
// - `elapsed(s)`: a turn's time on the line beside its step ("12s", "1m 59s").

// A chip about the web (web.rs), a page beside the chat (chat/pages.rs, and
// acting on one: chat/pages/act.rs) or the safety rule (chat/tools.rs).
const WEB_CHIP =
  /^(Searched the web|Couldn't search the web|Didn't search for|Didn't search the web for|Read [^"]|Couldn't read [^"]|Didn't read |Answered from |Brought up |Couldn't bring up |Didn't bring up |Took a shot of |Couldn't take a shot of |Didn't take a shot of |Offered to open |Didn't offer |Clicked |Typed in |Scrolled |Went to |Went (back|forward) in tab |Opened a search in tab |Showed tab |Closed tab |Listed the tabs |Looked at the page · |Zoomed in on the page · |Found (\d+ match(es)?|no match) for|Waited |Pressed |Hovered over |(Un)?[Tt]icked |Chose |Couldn't (click|type|scroll|go|switch|look at|zoom in|press|hover|set|open a tab|close tab|run|search|find|wait|act)|Didn't (click|type|press|go|open|act|switch|look|zoom|tick|run|use|see|find))/;
// The calendar's (chat/tools/calendar.rs).
const CALENDAR_CHIP = /^(Checked your calendar|Found \d+ meetings? with|No meetings with|Free |Busy |No calendar connected|Couldn't read the (day|time) )/;
// A document's (chat/documents.rs, and the rich tools': documents/rich.rs).
const DOC_CHIP =
  /^(Started "|(Re)?[Ww]rote (\d+ sections|"[^"]*") in "|Added "[^"]*" to "|Didn't (re)?write "|"[^"]*" is already written in "|Couldn't (start a document|rewrite a section)|Read (the document|what changed in) "|Changed (a table in|a part of) "|Added to "|Took a part out of "|Moved a part of "|Made \d+ changes in "|Didn't (change|restore) "|Couldn't (change|restore) ("|a document)|Looked at the history of "|Restored version \d+ of "|"[^"]*" is already as version )/;

/** A chip's mark: 'guard' | 'web' | 'calendar' | 'doc' | 'tool'. */
export const chipKind = (t) =>
  // (What Yap never does on a page, and a click the person said no to, too.)
  /: (not after reading the web|Yap never types .+|sign in yourself in the panel|you said no|no answer in time|couldn't ask you)$/.test(t)
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
  // Yap's free web sources don't cover it (web/sources.rs): nothing was
  // searched, by design — not a step that failed.
  [/^Didn't search the web for "/, 'notSearched'],
  [/^(Couldn't|Didn't) |: not after reading the web$/, 'failed'],
  [/^Searched the web/, 'web'],
  [/^Answered from (\d+) web pages?/, 'pages', (m) => Number(m[1])],
  [/^(Read (the document|what changed in) "|Looked at the history of ")/, 'readDoc'],
  [/^Read "/, 'readNote'],
  [/^Read .+ beside the chat$/, 'readBeside'],
  [/^Read /, 'pages'],
  [/^Started "/, 'doc'],
  [/^Rewrote (?:(\d+) sections|"[^"]*") in "/, 'rewrote', (m) => (m[1] ? Number(m[1]) : 1)],
  [/^[Ww]rote (?:(\d+) sections|"[^"]*") in "/, 'wrote', (m) => (m[1] ? Number(m[1]) : 1)],
  [/^(Added "[^"]*" to "|Added to "|Changed (a table in|a part of) "|Took a part out of "|Moved a part of "|Made \d+ changes in "|Restored version \d+ of ")/, 'docEdit'],
  [/^Clicked /, 'clicked'],
  [/^Typed in /, 'typed'],
  [/^((Un)?[Tt]icked |Chose )/, 'set'],
  [/^Pressed /, 'pressed'],
  [/^Hovered over /, 'hovered'],
  [/^Scrolled /, 'scrolled'],
  [/^(Went to |Went (back|forward) in tab |Opened a search in tab )/, 'went'],
  [/^Showed tab /, 'switched'],
  [/^(Closed tab |Listed the tabs )/, 'tabs'],
  [/^Looked at the page · /, 'pageLooked'],
  [/^Zoomed in on the page · /, 'zoomed'],
  [/^Found (\d+ match(es)?|no match) for /, 'found'],
  [/^Waited /, 'waited'],
  [/^Brought up a search for /, 'searchOpened'],
  [/^Brought up /, 'page'],
  [/^Took a shot of /, 'shot'],
  [/^Offered to open /, 'offered'],
  [/^Showed ‘/, 'shown'],
  [/^Looked at a picture/, 'looked'],
  [/^(Looked at (your screen|monitor )|Read your screen's text)/, 'screen'],
  [/^Found \d+ pictures? on /, 'pictures'],
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
  notSearched: () => "didn't search the web",
  pages: (n) => n1(n, 'read a page', 'read {n} pages'),
  readDoc: (n) => times(n, 'read the document'),
  readNote: (n) => n1(n, 'read a note', 'read {n} notes'),
  doc: (n) => n1(n, 'started a document', 'started {n} documents'),
  wrote: (n) => n1(n, 'wrote a section', 'wrote {n} sections'),
  rewrote: (n) => n1(n, 'rewrote a section', 'rewrote {n} sections'),
  docEdit: (n) => times(n, 'changed the document'),
  readBeside: (n) => n1(n, 'read the page beside the chat', 'read {n} pages beside the chat'),
  clicked: (n) => n1(n, 'clicked on the page', 'clicked {n} times on the page'),
  typed: (n) => n1(n, 'typed on the page', 'typed in {n} fields'),
  set: (n) => n1(n, 'set a field', 'set {n} fields'),
  pressed: (n) => n1(n, 'pressed keys', 'pressed keys {n} times'),
  hovered: (n) => n1(n, 'pointed at the page', 'pointed at the page {n} times'),
  scrolled: () => 'scrolled the page',
  went: (n) => n1(n, 'went to a page', 'went to {n} pages'),
  switched: (n) => n1(n, 'switched tabs', 'switched tabs {n} times'),
  tabs: () => 'worked the tabs',
  pageLooked: (n) => n1(n, 'looked at the page', 'looked at the page {n} times'),
  zoomed: (n) => n1(n, 'zoomed in', 'zoomed in {n} times'),
  found: (n) => times(n, 'looked for words on the page'),
  waited: () => 'waited for the page',
  searchOpened: (n) => n1(n, 'opened a search', 'opened {n} searches'),
  page: (n) => n1(n, 'brought up a page', 'brought up {n} pages'),
  shot: (n) => n1(n, 'took a shot of a page', 'took {n} shots of pages'),
  offered: (n) => n1(n, 'offered a page for your browser', 'offered {n} pages for your browser'),
  shown: (n) => n1(n, 'showed a picture', 'showed {n} pictures'),
  looked: (n) => n1(n, 'looked at a picture', 'looked at {n} pictures'),
  screen: (n) => times(n, 'looked at your screen'),
  pictures: (n) => times(n, 'looked for pictures'),
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

/**
 * **The shots the model took, on their steps** (chat/pages/see.rs: a
 * `ShownPicture` with its step's `chip`, "Looked at the page · example.com"):
 * for each chip, the next shot of that chip not yet placed, else null — the
 * k-th shot of a step's words is on the k-th step with them. `rest`: any
 * left over (a step's words changed since). Pure.
 */
export function stepShots(chips, shots) {
  const left = [...(shots || [])].filter((s) => s?.id && s.chip);
  const on = (chips || []).map((chip) => {
    const i = left.findIndex((s) => s.chip === String(chip).trim());
    return i < 0 ? null : left.splice(i, 1)[0];
  });
  return { on, rest: left };
}

/** The shots kept with an answer (its pictures with a step's chip), or as they come. Pure. */
export function shotsOf(m) {
  const kept = (m?.pictures || []).filter((p) => p?.chip);
  return kept.length ? kept : m?.liveShots || [];
}

/** A turn's time so far: "12s", "1m 59s", "1h 4m". Pure. */
export function elapsed(seconds) {
  const s = Math.max(0, Math.floor(seconds || 0));
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m ${s % 60}s`;
  return `${Math.floor(s / 3600)}h ${Math.floor((s % 3600) / 60)}m`;
}
