// The Notes view's menu items for a note's pin, its Version history and
// the saved transforms (notes/pins.rs, notes/versions.rs,
// notes/transform.rs), shared by the list's row menu (NoteList), the note's
// ⋯ (NoteMeta) and a right-click on selected text in My thoughts or the
// summary's box (NoteEditor, NoteSummary). Items as ui/ContextMenu.svelte
// takes them.

/** The selection in text box `el`, as a transform takes it, or null. */
export function selectionIn(el) {
  if (!el || el.selectionStart == null || el.selectionStart === el.selectionEnd) return null;
  const start = Math.min(el.selectionStart, el.selectionEnd);
  const end = Math.max(el.selectionStart, el.selectionEnd);
  const text = el.value.slice(start, end);
  return text.trim() ? { start, end, text } : null;
}

/** Pin or Unpin note `id`. */
export function pinItem(notes, id) {
  return notes.isPinned(id)
    ? { label: 'Unpin', icon: 'pin', onselect: () => notes.setPinned(id, false) }
    : { label: 'Pin', icon: 'pin', onselect: () => notes.setPinned(id, true) };
}

/** Version history… of note `id` (`returnFocus`: back there when it closes). */
export function historyItem(notes, id, returnFocus = null) {
  return { label: 'Version history…', icon: 'history', onselect: () => notes.openHistory(id, returnFocus) };
}

/**
 * The transforms submenu for the open note: `field` 'notes' (My thoughts) or
 * 'summary', on `selection` (`selectionIn`) or the whole text. The list is
 * the one loaded last (`notes.loadTransforms`); "Manage transforms…" opens
 * Settings → Commands.
 */
export function transformItem(notes, { field = 'notes', selection = null, openSettings = null } = {}) {
  const what = selection ? 'selection' : field === 'summary' ? 'summary' : 'note';
  const empty = field === 'summary' ? !notes.selected?.enhancedContent?.trim() : !notes.selected?.content?.trim();
  const busy = !!notes.transforming;
  const items = notes.transforms.map((t) => ({
    label: t.name,
    icon: 'sparkle',
    disabled: busy,
    onselect: () => notes.runTransform(t, { field, selection }),
  }));
  if (!items.length) items.push({ note: 'No transforms yet: add them in Settings → Commands.' });
  if (openSettings) {
    items.push({ separator: true }, { label: 'Manage transforms…', onselect: () => openSettings('commands#transforms') });
  }
  return {
    label: `Transform ${what}`,
    icon: 'sparkle',
    disabled: !selection && empty,
    items,
  };
}

/**
 * A right-click on text selected in a note's box (`el`): the transforms on
 * it, then Cut, Copy and Select all (the box gets the focus back first, its
 * selection kept, so the browser's own commands work and Ctrl+Z undoes them).
 */
export function selectionMenu(notes, el, { field = 'notes', openSettings = null } = {}) {
  const selection = selectionIn(el);
  if (!selection) return null;
  return [
    transformItem(notes, { field, selection, openSettings }),
    { separator: true },
    { label: 'Cut', icon: 'cut', disabled: el.readOnly, onselect: () => document.execCommand('cut') },
    { label: 'Copy', icon: 'copy', onselect: () => document.execCommand('copy') },
    { label: 'Select all', onselect: () => el.select() },
  ];
}
