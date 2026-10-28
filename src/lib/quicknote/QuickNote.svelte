<script>
  // The quick note (window `quick-note`, notes/quick_note.rs): a small window
  // on its own shortcut to jot or dictate a note from any app, saved as you
  // type as an ordinary note (the "Quick notes" folder in Notes). A plain
  // text box; the title is the note's first words until you name it in
  // Notes. New note, Open in Notes, Esc or ✕ to put it away. A dictation
  // while it's in front (or from its mic) lands at the cursor, never pasted
  // (`yap-quick-note-dictation`): Ctrl+Z takes it back out in one step.
  //
  // Rust says what to show (`yap-quick-note-open`: the last quick note
  // within a day, else a new one); the page saves (`quick_note_save`) one
  // save at a time, so a new note's first save is the only one that makes
  // it. Other windows follow its saves (`yap-note-changed`, origin
  // 'quick-note'), and it follows theirs. It has the in-page hotkey
  // fallback: a focused Yap window never sees the global hook.
  import { invoke } from '@tauri-apps/api/core';
  import { emit, listen } from '@tauri-apps/api/event';
  import { onMount, tick } from 'svelte';
  import Icon from '../notepad/NotepadIcon.svelte';
  import {
    actionMatchesKeydown,
    actionMatchesKeyup,
    formatHotkeySpec,
    hotkeyMatchesKeydown,
    hotkeyMatchesKeyup,
  } from '../core/hotkeys.js';
  import { readPageSelection } from '../stores/speech.svelte.js';
  import { languageShortcuts } from '../settings/languages.svelte.js';
  import { dictationPiece, withPiece, statusLine } from './quickNote.js';

  /** This window's label: the `origin` of its saves. */
  const LABEL = 'quick-note';
  /** Saved this long after the last keystroke. */
  const SAVE_MS = 400;

  // The note on screen. `cur.id` is its id once it has one (a new note's
  // first save gives it): every save goes to the note it was typed into,
  // even after the window moved on to another.
  let cur = { id: null };
  // `cur.id`, for what's drawn.
  let noteId = $state(null);
  let text = $state('');
  let title = $state('');
  let titleAuto = $state(false);
  let folder = $state('');
  // 'new' | 'saving' | 'saved' | 'unsaved'
  let saveState = $state('new');
  let cfg = $state(null);
  // The dictation pipeline's state (`yap-state`), for the mic button.
  let pipeline = $state('idle');
  let box = $state(null);

  let saveTimer = null;
  // Typed since the last save went out.
  let dirty = false;
  // The saves (and "a new note" for Rust), one after another: a new note's
  // first save is the only one that makes it.
  let chain = Promise.resolve();

  const recording = $derived(pipeline === 'recording');
  const dictateKey = $derived(cfg?.hotkey ? formatHotkeySpec(cfg.hotkey) : '');
  const shownTitle = $derived(title.trim() || 'New quick note');
  const canOpen = $derived(noteId != null || text.trim() !== '');

  function onInput() {
    dirty = true;
    saveState = 'saving';
    clearTimeout(saveTimer);
    saveTimer = setTimeout(flush, SAVE_MS);
  }

  /** Save what's typed now, for the note it's in (after any save on its way); resolves once it's saved. */
  function flush() {
    clearTimeout(saveTimer);
    saveTimer = null;
    if (dirty) {
      dirty = false;
      const target = cur;
      const content = text;
      chain = chain.then(() => saveFor(target, content));
    }
    return chain;
  }

  async function saveFor(target, content) {
    try {
      const s = await invoke('quick_note_save', { id: target.id, content });
      target.id = s?.noteId ?? null;
      // The window moved on to another note meanwhile: this was the old one's.
      if (target !== cur) return;
      noteId = target.id;
      title = s?.title || '';
      titleAuto = !!s?.titleAuto;
      if (s?.folder) folder = s.folder;
      if (s?.unsaved) saveState = 'unsaved';
      else if (!dirty && saveTimer === null) saveState = target.id == null ? 'new' : 'saved';
    } catch {
      // Not saved at all (the note store refused it): tried again with the next change.
      if (target === cur) {
        dirty = true;
        saveState = 'unsaved';
      }
    }
  }

  /** Every unsaved store written again (shell/saving.rs), then this note. */
  async function tryAgain() {
    await invoke('saves_retry').catch(() => {});
    dirty = true;
    await flush();
  }

  /** Show `p` (null: a new note), at once: what the last one had is saved first (`flush`). */
  function load(p) {
    clearTimeout(saveTimer);
    saveTimer = null;
    cur = { id: p?.noteId ?? null };
    noteId = cur.id;
    text = p?.content ?? '';
    title = p?.title ?? '';
    titleAuto = !!p?.titleAuto;
    folder = p?.folder ?? '';
    dirty = false;
    saveState = cur.id == null ? 'new' : 'saved';
  }

  /** The cursor into the note, at its end (a note opened). */
  async function focusEnd() {
    await tick();
    if (!box) return;
    box.focus();
    const end = box.value.length;
    box.setSelectionRange(end, end);
    box.scrollTop = box.scrollHeight;
  }

  // Rust opened the window: on this note, a new one ("New quick note"), or
  // the one it's on.
  function onOpen(p) {
    if (p?.fresh) {
      newNote();
      return;
    }
    if (p?.noteId != null && p.noteId === cur.id) {
      // Already on it: what's here is newer.
    } else if (cur.id == null && text.trim()) {
      // A new note being written: it stays (its first save makes it).
      flush();
    } else {
      flush();
      load(p);
    }
    focusEnd();
  }

  /** A new note, at once (the last one saved first): typing can start straight away. */
  function newNote() {
    flush();
    chain = chain.then(() => invoke('quick_note_new').catch(() => {}));
    load(null);
    focusEnd();
  }

  /** Esc, ✕: away (the save carries on). */
  function close() {
    flush();
    invoke('quick_note_hide').catch(() => {});
  }

  /** The note in the main window's Notes view; this window steps aside. */
  async function openInNotes() {
    const target = cur;
    await flush();
    if (target.id == null) return;
    await invoke('open_settings').catch(() => {});
    await emit('yap-meeting-open-note', { noteId: target.id, stop: false }).catch(() => {});
    invoke('quick_note_hide').catch(() => {});
  }

  /** Its mic: a dictation into the note, or stop the one recording. */
  function dictate() {
    box?.focus();
    invoke('quick_note_dictate').catch(() => {});
  }

  // A dictation for the note (dictation/pipeline/delivery.rs `deliver_to_note`):
  // at the cursor, as typing would put it, so Ctrl+Z takes it back out.
  function onDictation(p) {
    const el = box;
    if (!el) return;
    const at = document.activeElement === el;
    const start = at ? el.selectionStart : text.length;
    const end = at ? el.selectionEnd : text.length;
    const piece = dictationPiece(text.slice(0, start), text.slice(end), p?.text, !!p?.newline);
    if (!piece) return;
    const want = withPiece(text, start, end, piece);
    el.focus();
    el.setSelectionRange(start, end);
    // `insertText` keeps the box's own undo; a window without the focus
    // (a test run, or put away meanwhile) may refuse it: then by hand.
    let done = false;
    try {
      done = document.execCommand('insertText', false, piece) && el.value === want.value;
    } catch {
      done = false;
    }
    if (!done) {
      text = want.value;
      tick().then(() => el.setSelectionRange(want.caret, want.caret));
      onInput();
    }
  }

  // Another window changed this note (the Notes view): its words and title,
  // unless something's being typed here.
  function onNoteChanged(p) {
    if (!p || p.origin === LABEL || p.id == null || p.id !== cur.id) return;
    if (!dirty && saveTimer === null) {
      if (typeof p.content === 'string') text = p.content;
    }
    title = p.title || '';
    titleAuto = !!p.titleAuto;
    if (p.folder) folder = p.folder;
  }

  // Deleted elsewhere (Notes): the window lets go of it. What's being typed
  // stays, and makes a new note.
  function onNoteDeleted(p) {
    if (p?.id == null || p.id !== cur.id) return;
    cur.id = null;
    noteId = null;
    title = '';
    titleAuto = false;
    if (dirty || saveTimer !== null) return;
    load(null);
  }

  async function refreshConfig() {
    try {
      cfg = await invoke('get_config');
    } catch {
      /* keep */
    }
  }

  // ---- keys: the note's own, then the in-page hotkey fallback (the WebView2-focus gotcha) ----

  function onKeyDown(e) {
    if (e.isComposing) return;
    if (e.key === 'Escape' && !e.repeat) {
      e.preventDefault();
      close();
      return;
    }
    if ((e.ctrlKey || e.metaKey) && !e.altKey && !e.shiftKey && e.key.toLowerCase() === 'n') {
      e.preventDefault();
      if (!e.repeat) newNote();
      return;
    }
    if (e.repeat || !cfg) return;
    const lang = languageShortcuts(cfg).find((s) => hotkeyMatchesKeydown(e, s.hotkey));
    // The Quick note shortcut while it's in front: away.
    if (actionMatchesKeydown(e, cfg, 'quickNoteHotkey')) {
      e.preventDefault();
      e.stopPropagation();
      flush();
      invoke('quick_note_toggle').catch(() => {});
    } else if (actionMatchesKeydown(e, cfg, 'meetingHotkey')) {
      e.preventDefault();
      e.stopPropagation();
      flush();
      invoke('meeting_shortcut', { origin: LABEL }).catch(() => {});
    } else if (actionMatchesKeydown(e, cfg, 'assistantHotkey')) {
      e.preventDefault();
      e.stopPropagation();
      invoke('assistant_key', { pressed: true }).catch(() => {});
    } else if (actionMatchesKeydown(e, cfg, 'readHotkey')) {
      // Read aloud (voice/read_aloud.rs): what's selected here.
      e.preventDefault();
      e.stopPropagation();
      readPageSelection();
    } else if (actionMatchesKeydown(e, cfg, 'snipHotkey')) {
      e.preventDefault();
      e.stopPropagation();
      invoke('snip_start', { origin: 'shortcut' }).catch(() => {});
    } else if (actionMatchesKeydown(e, cfg, 'copyLastHotkey')) {
      e.preventDefault();
      e.stopPropagation();
      invoke('copy_last_transcript').catch(() => {});
    } else if (lang) {
      // A language shortcut (My languages): a dictation in its language, into the note.
      e.preventDefault();
      e.stopPropagation();
      invoke('language_key', { id: lang.id, pressed: true }).catch(() => {});
    } else if (actionMatchesKeydown(e, cfg, 'hotkey')) {
      // Dictation: Rust sees this window in front, so it's the note's.
      e.preventDefault();
      e.stopPropagation();
      invoke('dictation_key', { pressed: true }).catch(() => {});
    }
  }
  // The key coming up: push-to-talk's release (Rust decides by the mode).
  function onKeyUp(e) {
    if (!cfg) return;
    const lang = languageShortcuts(cfg).find((s) => hotkeyMatchesKeyup(e, s.hotkey));
    if (actionMatchesKeyup(e, cfg, 'assistantHotkey')) {
      e.preventDefault();
      invoke('assistant_key', { pressed: false }).catch(() => {});
    } else if (lang) {
      e.preventDefault();
      invoke('language_key', { id: lang.id, pressed: false }).catch(() => {});
    } else if (actionMatchesKeyup(e, cfg, 'hotkey')) {
      e.preventDefault();
      invoke('dictation_key', { pressed: false }).catch(() => {});
    }
  }

  onMount(() => {
    refreshConfig();
    invoke('quick_note_state')
      .then((s) => {
        // Not if the person started typing while it loaded.
        if (cur.id == null && !text) load(s);
      })
      .catch(() => {});
    window.addEventListener('keydown', onKeyDown, true);
    window.addEventListener('keyup', onKeyUp, true);
    // Saved on the way out, whatever closes the page.
    const leaving = () => flush();
    window.addEventListener('beforeunload', leaving);
    const uns = [];
    const on = (name, fn) => listen(name, (e) => fn(e.payload)).then((u) => uns.push(u));
    on('yap-quick-note-open', onOpen);
    on('yap-quick-note-visible', (shown) => {
      if (shown) refreshConfig();
      else flush();
    });
    on('yap-quick-note-dictation', onDictation);
    on('yap-note-changed', onNoteChanged);
    on('yap-note-deleted', onNoteDeleted);
    on('yap-config-saved', refreshConfig);
    on('yap-state', (s) => (pipeline = String(s || 'idle')));
    return () => {
      flush();
      window.removeEventListener('keydown', onKeyDown, true);
      window.removeEventListener('keyup', onKeyUp, true);
      window.removeEventListener('beforeunload', leaving);
      uns.forEach((u) => u());
    };
  });
</script>

<main class="qn" aria-label="Quick note">
  <header class="hdr" data-tauri-drag-region>
    <span class="title" class:muted={!title.trim() || titleAuto} title={shownTitle} data-tauri-drag-region>{shownTitle}</span>
    <div class="tools">
      <button class="iconbtn" aria-label="New note" title="New note (Ctrl+N)" onclick={newNote}>
        <Icon name="plus" />
      </button>
      <button class="iconbtn" aria-label="Open in Notes" title="Open in Notes" disabled={!canOpen} onclick={openInNotes}>
        <Icon name="open" />
      </button>
      <button class="iconbtn" aria-label="Close" title="Close (Esc)" onclick={close}>
        <Icon name="close" />
      </button>
    </div>
  </header>

  <textarea
    class="note"
    bind:this={box}
    bind:value={text}
    oninput={onInput}
    aria-label="Quick note"
    placeholder={dictateKey ? `Jot something down, or press ${dictateKey} and talk.` : 'Jot something down.'}
    spellcheck="true"
  ></textarea>

  <footer class="foot">
    <button
      class="mic"
      class:on={recording}
      aria-label={recording ? 'Stop dictating' : 'Dictate into the note'}
      aria-pressed={recording}
      title={recording ? 'Stop dictating' : 'Dictate into the note'}
      onclick={dictate}
    >
      <Icon name={recording ? 'stop' : 'mic'} />
    </button>
    <span class="status" class:warn={saveState === 'unsaved'} role="status">{statusLine(saveState, folder)}</span>
    {#if saveState === 'unsaved'}
      <button class="retry" onclick={tryAgain}>Try again</button>
    {/if}
  </footer>
</main>

<style>
  button {
    font: inherit;
    cursor: pointer;
  }
  .qn {
    display: flex;
    flex-direction: column;
    height: 100vh;
    background: var(--yap-paper);
    color: var(--yap-paper-ink);
    font-size: 14px;
  }

  /* The window has no frame: the header is its drag region. */
  .hdr {
    flex: 0 0 40px;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 6px 0 16px;
    user-select: none;
    -webkit-user-select: none;
  }
  .title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    font-weight: 650;
    font-size: 13.5px;
  }
  .title.muted {
    color: var(--yap-paper-muted);
    font-weight: 550;
  }
  .tools {
    display: flex;
    align-items: center;
    gap: 2px;
  }
  .iconbtn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 30px;
    padding: 0;
    border: none;
    border-radius: 8px;
    background: transparent;
    color: var(--yap-paper-ink);
    transition: background-color var(--yap-dur);
  }
  .iconbtn :global(svg) {
    width: 16px;
    height: 16px;
  }
  .iconbtn:hover:not(:disabled) {
    background: var(--yap-paper-s2);
  }
  .iconbtn:disabled {
    color: var(--yap-paper-faint);
    cursor: default;
  }
  .iconbtn:focus-visible,
  .mic:focus-visible,
  .retry:focus-visible {
    outline: none;
    box-shadow: 0 0 0 3px var(--yap-paper-ring);
  }

  .note {
    flex: 1 1 auto;
    min-height: 0;
    margin: 0;
    padding: 4px 18px 12px;
    border: none;
    background: transparent;
    color: var(--yap-paper-ink);
    /* Reading text: Settings → General → Appearance (app.css). */
    font-family: var(--yap-font-reading);
    font-size: calc(15px * var(--yap-reading-scale));
    line-height: calc(1.55 * var(--yap-reading-leading));
    resize: none;
    outline: none;
    user-select: text;
    -webkit-user-select: text;
  }
  .note::placeholder {
    color: var(--yap-paper-date);
  }

  .foot {
    flex: 0 0 40px;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 12px;
    border-top: 1px solid var(--yap-paper-s2);
    user-select: none;
    -webkit-user-select: none;
  }
  .mic {
    flex: 0 0 auto;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    padding: 0;
    border: none;
    border-radius: var(--yap-r-full);
    background: var(--yap-paper-s2);
    color: var(--yap-paper-ink);
    transition: background-color var(--yap-dur);
  }
  .mic :global(svg) {
    width: 15px;
    height: 15px;
  }
  .mic:hover {
    background: var(--yap-paper-s2-strong);
  }
  .mic.on {
    color: var(--yap-live-strong);
  }
  .status {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    color: var(--yap-paper-muted);
    font-size: 12.5px;
  }
  .status.warn {
    color: var(--yap-paper-snag);
  }
  .retry {
    flex: 0 0 auto;
    height: 26px;
    padding: 0 10px;
    border: none;
    border-radius: 7px;
    background: var(--yap-paper-s2);
    color: var(--yap-paper-ink);
    font-size: 12.5px;
    font-weight: 600;
  }
  .retry:hover {
    background: var(--yap-paper-s2-strong);
  }
</style>
