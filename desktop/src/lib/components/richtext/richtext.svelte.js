// A note's page, loaded on demand (2026-10-31): `RichNote.svelte` and the
// rich text editor under it (TipTap on ProseMirror, ~135 kB gzipped) are
// chunks of their own, imported the first time a window shows a note to
// edit — the meeting notepad with its meeting, the quick note (`loadRichtext`),
// the Notes view with a note open (its own `NoteThoughts` page, through
// `loadPage`) — never with a window's first paint (the Yap bar's and the
// main window's chunks don't carry them). Until one's in (and if it can't
// load) the note's plain text box stands in, its `data-rich-fallback`
// marking it: the caret in it carries over to the page (`takeHandoff`).
//
// `richtext.Note`: RichNote once loaded (null until then).

export const richtext = $state({ Note: null });

let loading = null;
// The plain box's selection as the page took its place: `{start, end}`.
let handoff = null;

/** The plain box in use now hands its caret over to the page coming in. */
function handOver() {
  const a = typeof document !== 'undefined' ? document.activeElement : null;
  handoff = a?.matches?.('textarea[data-rich-fallback]') ? { start: a.selectionStart, end: a.selectionEnd } : null;
}

/** A page's component, `importer()`'s chunk loaded (the plain box's caret handed over); null if it can't load. */
export function loadPage(importer) {
  return importer()
    .then((m) => {
      handOver();
      return m.default;
    })
    .catch(() => null);
}

/** Start loading RichNote (once; again only after a failure); resolves when it's in, or null. */
export function loadRichtext() {
  if (richtext.Note) return Promise.resolve(richtext.Note);
  loading ??= loadPage(() => import('./RichNote.svelte')).then((c) => {
    // Failed: the plain box stays, and the next note shown tries again.
    if (c) richtext.Note = c;
    else loading = null;
    return c;
  });
  return loading;
}

/** The caret the plain box handed over (once), or null. */
export function takeHandoff() {
  const h = handoff;
  handoff = null;
  return h;
}
