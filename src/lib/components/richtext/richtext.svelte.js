// The note page's editor, loaded on demand (2026-10-31): `RichNote.svelte`
// and the rich text editor under it (TipTap on ProseMirror, ~133 kB
// gzipped) are a chunk of their own, imported the first time a window
// shows a note to edit — the Notes view with a note open, the meeting
// notepad with its meeting, the quick note — never with a window's first
// paint (the Yap bar's and the main window's chunks don't carry it). Until
// it's in (and if it can't load) the note's plain text box stands in, its
// `data-rich-fallback` marking it: the caret in it carries over to the page.
//
// `richtext.Note`: the component once loaded (null until then);
// `loadRichtext()`: start loading it (again only if it failed).

export const richtext = $state({ Note: null, failed: false });

let loading = null;
// The plain box's selection as the page took its place: `{start, end}`.
let handoff = null;

/** Start loading the note page (once); resolves when it's in, or failed. */
export function loadRichtext() {
  if (richtext.Note) return Promise.resolve(richtext.Note);
  loading ??= import('./RichNote.svelte')
    .then((m) => {
      // The plain box in use now hands its caret over to the page.
      const a = typeof document !== 'undefined' ? document.activeElement : null;
      handoff = a?.matches?.('textarea[data-rich-fallback]') ? { start: a.selectionStart, end: a.selectionEnd } : null;
      richtext.failed = false;
      richtext.Note = m.default;
      return m.default;
    })
    .catch(() => {
      // The plain box stays; the next note shown tries again.
      richtext.failed = true;
      loading = null;
      return null;
    });
  return loading;
}

/** The caret the plain box handed over (once), or null. */
export function takeHandoff() {
  const h = handoff;
  handoff = null;
  return h;
}
