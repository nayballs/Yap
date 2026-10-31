<script>
  // The Notes view's My thoughts as a page you can format (2026-10-31;
  // components/richtext/RichNote: the toolbar, Preview / Source, Ctrl+F),
  // the note kept markdown. Its own chunk with the editor, loaded the first
  // time a note is open here (NoteEditor shows the note's plain box until
  // then), so the main window's first paint carries none of it. Saved as
  // the box's typing was (a paste a dictation's, notes/versions.rs); the
  // selection's right-click and Shift+F10 menu (noteMenus.js) as the box
  // had it: the page answers as a text box does (`notes.rawEl`).
  import RichNote from '../../components/richtext/RichNote.svelte';
  import { openContextMenu, menuKey } from '../../ui/contextMenu.svelte.js';
  import { selectionMenu } from './noteMenus.js';

  /** `notes`: the view's state; `onopensettings(section)`: Settings (the menu's "Manage transforms…"). */
  let { notes, onopensettings = null } = $props();

  // A transform on My thoughts runs: the page waits (notes/transform.rs).
  const busy = $derived(notes.transforming?.noteId === notes.selected?.id && notes.transforming?.field === 'notes');
  const placeholder = $derived(
    notes.recordingThisNote ? 'Take rough notes while Yap listens — they get woven into the minutes…' : 'Start writing…'
  );

  /** Typed on the page (or its Source): saved as the box's typing is; a paste may be a dictation. */
  function onchange(md, how) {
    if (!notes.selected) return;
    notes.selected.content = md;
    notes.queueSave('content', how?.paste ? { inputType: 'insertFromPaste' } : null);
  }

  // Selected words: the transforms, Cut, Copy, Select all; else the page's own menu.
  function onmenu(e) {
    const items = selectionMenu(notes, e.currentTarget, { field: 'notes', openSettings: onopensettings });
    if (items) openContextMenu(e, items, { label: 'Selected text', row: e.currentTarget });
  }
</script>

<!-- One editor per note (its own undo history). -->
{#key notes.selected?.id}
  <RichNote
    text={notes.selected?.content ?? ''}
    {onchange}
    label="My thoughts"
    {placeholder}
    readonly={busy}
    look="app"
    canSource
    sourceKey="yapNoteView"
    bind:el={notes.rawEl}
    oncontextmenu={onmenu}
    onkeydown={(e) => menuKey(e, onmenu)}
    --rich-size="calc(13px * var(--yap-reading-scale))"
    --rich-leading="calc(1.7 * var(--yap-reading-leading))"
    --rich-pad="10px 2px 16px"
  />
{/key}
