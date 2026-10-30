<script>
  // The quick note (window `quick-note`, notes/quick_note.rs): a small window
  // on its own shortcut to jot or dictate a note from any app, saved as you
  // type as an ordinary note (the "Quick notes" folder in Notes). A plain
  // text box; the title is the note's first words until you name it in
  // Notes. A dictation while it's in front (or from its mic) lands at the
  // cursor, never pasted (`yap-quick-note-dictation`): Ctrl+Z takes it back
  // out in one step.
  //
  // Several quick notes open as **tabs** (quick_note/tabs.rs): + or Ctrl+T
  // (Ctrl+N) a new one, × or Ctrl+W to close, Ctrl+Tab to switch; Rust is
  // told the open notes after every change and remembers them across
  // restarts. **Send to {app}** (quick_note/send.rs) puts the note, or its
  // selection, into the app that was in front when it came up. On an empty
  // page it offers a quick-note button on the Yap bar (`bar_quick_note`).
  //
  // Rust says what to show (`yap-quick-note-open`: the last quick note
  // within a day, else a new one, and which tabs close); the page saves
  // (`quick_note_save`) one save at a time, so a new note's first save is
  // the only one that makes it. Other windows follow its saves
  // (`yap-note-changed`, origin 'quick-note'), and it follows theirs. It has
  // the in-page hotkey fallback: a focused Yap window never sees the global
  // hook — the Quick note shortcut's press and release included (its hold
  // dictates into the note, quick_note/key.rs).
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
  import { appLabel } from '../core/appNames.js';
  import { readPageSelection } from '../stores/speech.svelte.js';
  import { languageShortcuts } from '../settings/languages.svelte.js';
  import { dictationPiece, withPiece, statusLine, tabLabel, nextTab, afterClose, sendText } from './quickNote.js';

  /** This window's label: the `origin` of its saves. */
  const LABEL = 'quick-note';
  /** Saved this long after the last keystroke. */
  const SAVE_MS = 400;
  /** "Not now" on the bar offer, remembered in this window only. */
  const OFFER_KEY = 'yap.quickNote.barOfferDismissed';

  // The open notes, in order: `{ key, id, title, titleAuto, folder, text }`.
  // `id` is the note's once it has one (a new note's first save gives it):
  // every save goes to the tab it was typed in, even after another came to
  // the front.
  let tabs = $state([]);
  let activeKey = $state(null);
  let seq = 0;
  // The tab in front (one of `tabs`).
  let cur = null;
  // The tab in front, for what's drawn.
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
  // Where Send goes (`{ app: 'slack.exe', icon }`), or null.
  let sendTo = $state(null);
  let offerDismissed = $state(readDismissed());

  let saveTimer = null;
  // Typed since the last save went out.
  let dirty = false;
  // The saves (and the tabs for Rust), one after another: a new note's first
  // save is the only one that makes it.
  let chain = Promise.resolve();

  const recording = $derived(pipeline === 'recording');
  const dictateKey = $derived(cfg?.hotkey ? formatHotkeySpec(cfg.hotkey) : '');
  const shownTitle = $derived(title.trim() || 'New quick note');
  const canOpen = $derived(noteId != null || text.trim() !== '');
  const sendName = $derived(sendTo ? appLabel(sendTo.app) : '');
  const empty = $derived(text.trim() === '');
  const offerBar = $derived(!!cfg && cfg.barEnabled !== false && !cfg.barQuickNote && !offerDismissed && empty);

  function readDismissed() {
    try {
      return localStorage.getItem(OFFER_KEY) === '1';
    } catch {
      return false;
    }
  }

  // ---- the tabs ----

  function makeTab(p) {
    return {
      key: ++seq,
      id: p?.noteId ?? null,
      title: p?.title ?? '',
      titleAuto: !!p?.titleAuto,
      folder: p?.folder ?? '',
      text: p?.content ?? '',
    };
  }

  /** A tab for `p` (null: a new note) at the end; the tab (as `tabs` holds it). */
  function addTab(p) {
    tabs.push(makeTab(p));
    return tabs[tabs.length - 1];
  }

  const indexOf = (tab) => tabs.findIndex((t) => t.key === tab?.key);

  /** Put `tab` in front at once (what was in front is saved first: `leave`). */
  function show(tab) {
    clearTimeout(saveTimer);
    saveTimer = null;
    cur = tab;
    activeKey = tab.key;
    noteId = tab.id;
    text = tab.text;
    title = tab.title;
    titleAuto = tab.titleAuto;
    folder = tab.folder;
    dirty = false;
    saveState = tab.id == null ? 'new' : 'saved';
  }

  // The tab in front in view, whole, when there are more than fit (again
  // as its name grows).
  $effect(() => {
    void activeKey;
    void title;
    tick().then(() =>
      requestAnimationFrame(() => document.querySelector('.tab.on')?.scrollIntoView?.({ block: 'nearest', inline: 'nearest' }))
    );
  });

  /** What's in front, saved (its words kept in its tab). */
  function leave() {
    if (cur) cur.text = text;
    flush();
  }

  /** Rust told the open notes and the one in front (after any save on its way). */
  function rememberTabs() {
    chain = chain.then(() =>
      invoke('quick_note_tabs', {
        ids: tabs.filter((t) => t.id != null).map((t) => t.id),
        active: cur?.id ?? null,
      }).catch(() => {})
    );
  }

  function switchTo(key) {
    const tab = tabs.find((t) => t.key === key);
    if (!tab) return;
    if (tab !== cur) {
      leave();
      show(tab);
      rememberTabs();
    }
    focusEnd();
  }

  /** A new note in a tab of its own (the blank one there, if there is one): typing can start at once. */
  function newTab() {
    if (cur && cur.id == null && !text.trim()) {
      focusEnd();
      return;
    }
    leave();
    const blank = tabs.find((t) => t !== cur && t.id == null && !t.text.trim());
    show(blank ?? addTab(null));
    rememberTabs();
    focusEnd();
  }

  /** Close a tab (its note stays in Notes); the last one closed leaves a new note. */
  function closeTab(key) {
    const i = tabs.findIndex((t) => t.key === key);
    if (i < 0) return;
    const front = tabs[i] === cur;
    if (front) leave();
    const count = tabs.length;
    tabs.splice(i, 1);
    if (front) {
      show(count > 1 ? tabs[afterClose(count, i)] : addTab(null));
      focusEnd();
    }
    rememberTabs();
  }

  function cycle(step) {
    if (tabs.length < 2) return;
    switchTo(tabs[nextTab(tabs.length, indexOf(cur), step)].key);
  }

  /** Remembered tabs Rust closed (left alone a day, gone): never the one in front. */
  function closeStale(ids) {
    let changed = false;
    for (const id of ids || []) {
      const i = tabs.findIndex((t) => t.id === id);
      if (i >= 0 && tabs[i] !== cur) {
        tabs.splice(i, 1);
        changed = true;
      }
    }
    if (changed) rememberTabs();
  }

  // ---- saving ----

  function onInput() {
    if (cur) cur.text = text;
    dirty = true;
    saveState = 'saving';
    clearTimeout(saveTimer);
    saveTimer = setTimeout(flush, SAVE_MS);
  }

  /** Save what's typed now, for the tab it's in (after any save on its way); resolves once it's saved. */
  function flush() {
    clearTimeout(saveTimer);
    saveTimer = null;
    if (dirty && cur) {
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
      const had = target.id;
      target.id = s?.noteId ?? null;
      target.title = s?.title || '';
      target.titleAuto = !!s?.titleAuto;
      if (s?.folder) target.folder = s.folder;
      // A note made or let go: the open notes Rust remembers change.
      if (had !== target.id) rememberTabs();
      // Another tab is in front now: this was the old one's.
      if (target !== cur) return;
      noteId = target.id;
      title = target.title;
      titleAuto = target.titleAuto;
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

  /** The cursor into the note, at its end (a note opened). */
  async function focusEnd() {
    await tick();
    if (!box) return;
    box.focus();
    const end = box.value.length;
    box.setSelectionRange(end, end);
    box.scrollTop = box.scrollHeight;
  }

  // Rust opened the window: on this note (its tab, or a new tab for it), a
  // new one ("New quick note", or nothing recent), and which remembered
  // tabs close now.
  function onOpen(p) {
    if (p?.fresh) {
      newTab();
    } else if (p?.noteId != null) {
      const tab = tabs.find((t) => t.id === p.noteId);
      if (tab) {
        // Already in front: what's here is newer.
        if (tab !== cur) {
          leave();
          show(tab);
          rememberTabs();
        }
      } else {
        leave();
        show(addTab(p));
        rememberTabs();
      }
    } else if (!(cur && cur.id == null)) {
      // Nothing recent: a new note (one being written in front stays).
      newTab();
    }
    closeStale(p?.close);
    focusEnd();
    refreshSendTarget();
  }

  /** The page loading: the remembered tabs, on the note Rust picked (else a new one). */
  function onState(s) {
    // Not if the person started typing while it loaded.
    if (tabs.length > 1 || cur?.id != null || text) return;
    const open = (s?.tabs || []).map(makeTab);
    if (!open.length && s?.noteId == null) return;
    tabs = open;
    const front = s?.noteId != null ? tabs.find((t) => t.id === s.noteId) : null;
    show(front ?? addTab(null));
    rememberTabs();
  }

  /** Esc, ✕: away (the save carries on). */
  function close() {
    leave();
    invoke('quick_note_hide').catch(() => {});
  }

  /** The note in the main window's Notes view; this window steps aside. */
  async function openInNotes() {
    const target = cur;
    leave();
    await flush();
    if (target?.id == null) return;
    await invoke('open_settings').catch(() => {});
    await emit('yap-meeting-open-note', { noteId: target.id, stop: false }).catch(() => {});
    invoke('quick_note_hide').catch(() => {});
  }

  /** Its mic: a dictation into the note, or stop the one recording. */
  function dictate() {
    box?.focus();
    invoke('quick_note_dictate').catch(() => {});
  }

  // ---- Send to {app} ----

  async function refreshSendTarget() {
    try {
      sendTo = await invoke('quick_note_send_target');
    } catch {
      sendTo = null;
    }
  }

  /** The note (or its selection) into the app it came from; the window steps aside. */
  async function send() {
    const piece = sendText(text, box?.selectionStart, box?.selectionEnd);
    if (!piece.trim()) return;
    leave();
    await flush();
    invoke('quick_note_send', { text: piece }).catch((e) => {
      invoke('frontend_log', { msg: `quick note: send failed: ${e}` }).catch(() => {});
      refreshSendTarget();
    });
  }

  // ---- the bar's button ----

  function addToBar() {
    invoke('quick_note_bar_button', { on: true }).catch(() => {});
  }

  function notNow() {
    offerDismissed = true;
    try {
      localStorage.setItem(OFFER_KEY, '1');
    } catch {
      /* this session only */
    }
    box?.focus();
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

  // Another window changed one of these notes (the Notes view): its words
  // and title — in front, unless something's being typed here.
  function onNoteChanged(p) {
    if (!p || p.origin === LABEL || p.id == null) return;
    const tab = tabs.find((t) => t.id === p.id);
    if (!tab) return;
    tab.title = p.title || '';
    tab.titleAuto = !!p.titleAuto;
    if (p.folder) tab.folder = p.folder;
    if (tab !== cur) {
      if (typeof p.content === 'string') tab.text = p.content;
      return;
    }
    if (!dirty && saveTimer === null && typeof p.content === 'string') {
      text = p.content;
      tab.text = p.content;
    }
    title = tab.title;
    titleAuto = tab.titleAuto;
    if (p.folder) folder = p.folder;
  }

  // Deleted elsewhere (Notes): a tab behind closes; the one in front lets go
  // of it — what's being typed stays, and makes a new note.
  function onNoteDeleted(p) {
    if (p?.id == null) return;
    const tab = tabs.find((t) => t.id === p.id);
    if (!tab) return;
    if (tab !== cur) {
      tabs.splice(indexOf(tab), 1);
      rememberTabs();
      return;
    }
    cur.id = null;
    cur.title = '';
    cur.titleAuto = false;
    noteId = null;
    title = '';
    titleAuto = false;
    if (!dirty && saveTimer === null) {
      cur.text = '';
      text = '';
      saveState = 'new';
    }
    rememberTabs();
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
    const ctrl = (e.ctrlKey || e.metaKey) && !e.altKey;
    // Ctrl+Tab / Ctrl+Shift+Tab (and Ctrl+PgDn / PgUp): the next tab, the one before.
    if (ctrl && (e.key === 'Tab' || e.key === 'PageDown' || e.key === 'PageUp')) {
      e.preventDefault();
      cycle(e.key === 'PageUp' || (e.key === 'Tab' && e.shiftKey) ? -1 : 1);
      return;
    }
    if (ctrl && !e.shiftKey && ['n', 't', 'w'].includes(e.key.toLowerCase())) {
      e.preventDefault();
      if (e.repeat) return;
      if (e.key.toLowerCase() === 'w') closeTab(activeKey);
      else newTab();
      return;
    }
    if (e.repeat || !cfg) return;
    const lang = languageShortcuts(cfg).find((s) => hotkeyMatchesKeydown(e, s.hotkey));
    // The Quick note shortcut while it's in front: a tap puts it away, a
    // hold dictates into it (quick_note/key.rs decides on the release).
    if (actionMatchesKeydown(e, cfg, 'quickNoteHotkey')) {
      e.preventDefault();
      e.stopPropagation();
      leave();
      invoke('quick_note_key', { pressed: true }).catch(() => {});
    } else if (actionMatchesKeydown(e, cfg, 'meetingHotkey')) {
      e.preventDefault();
      e.stopPropagation();
      leave();
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
  // The key coming up: the Quick note shortcut's (tap or hold, pressed here
  // or in another app — this window took the focus on the press), and
  // push-to-talk's release (Rust decides by the mode).
  function onKeyUp(e) {
    if (!cfg) return;
    const lang = languageShortcuts(cfg).find((s) => hotkeyMatchesKeyup(e, s.hotkey));
    // Its own key has a hold of its own (quick_note/key.rs), the others
    // share the dictation's: a key up ending both (a modifier in each) goes
    // to both, or push-to-talk keeps recording.
    if (actionMatchesKeyup(e, cfg, 'quickNoteHotkey')) {
      e.preventDefault();
      invoke('quick_note_key', { pressed: false }).catch(() => {});
    }
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
    // A new note at once, so typing can start before Rust answers.
    show(addTab(null));
    refreshConfig();
    invoke('quick_note_state').then(onState).catch(() => {});
    window.addEventListener('keydown', onKeyDown, true);
    window.addEventListener('keyup', onKeyUp, true);
    // Saved on the way out, whatever closes the page.
    const leaving = () => leave();
    window.addEventListener('beforeunload', leaving);
    const uns = [];
    const on = (name, fn) => listen(name, (e) => fn(e.payload)).then((u) => uns.push(u));
    on('yap-quick-note-open', onOpen);
    on('yap-quick-note-visible', (shown) => {
      if (shown) {
        refreshConfig();
        refreshSendTarget();
      } else {
        leave();
      }
    });
    on('yap-quick-note-dictation', onDictation);
    on('yap-note-changed', onNoteChanged);
    on('yap-note-deleted', onNoteDeleted);
    on('yap-config-saved', refreshConfig);
    on('yap-state', (s) => (pipeline = String(s || 'idle')));
    return () => {
      leave();
      window.removeEventListener('keydown', onKeyDown, true);
      window.removeEventListener('keyup', onKeyUp, true);
      window.removeEventListener('beforeunload', leaving);
      uns.forEach((u) => u());
    };
  });
</script>

<main class="qn" aria-label="Quick note">
  <header class="hdr" data-tauri-drag-region>
    <div class="tabs" role="tablist" aria-label="Quick notes" data-tauri-drag-region>
      {#each tabs as t (t.key)}
        {@const on = t.key === activeKey}
        <div class="tab" class:on data-tauri-drag-region>
          <button
            role="tab"
            class="tablabel"
            class:title={on}
            class:muted={on ? !title.trim() || titleAuto : !t.title.trim() || t.titleAuto}
            aria-selected={on}
            title={on ? shownTitle : tabLabel(t)}
            onclick={() => switchTo(t.key)}
          >{on ? shownTitle : tabLabel(t)}</button>
          <button class="tabx" aria-label={`Close ${on ? shownTitle : tabLabel(t)}`} title="Close tab (Ctrl+W)" onclick={() => closeTab(t.key)}>
            <Icon name="x" />
          </button>
        </div>
      {/each}
    </div>
    <!-- Outside the strip, so it stays in view however many tabs there are. -->
    <button class="iconbtn add" aria-label="New note" title="New note (Ctrl+T)" onclick={newTab}>
      <Icon name="plus" />
    </button>
    <span class="grow" data-tauri-drag-region></span>
    <div class="tools">
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

  {#if offerBar}
    <div class="offer" role="group" aria-label="The Yap bar">
      <span class="offertext">Open it from the Yap bar too</span>
      <button class="offerbtn" onclick={addToBar}>Add to the Yap bar</button>
      <button class="offerx" aria-label="Not now" title="Not now" onclick={notNow}><Icon name="x" /></button>
    </div>
  {/if}

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
    {#if sendTo}
      <button
        class="send"
        disabled={empty}
        title={`Paste the note, or what's selected in it, into ${sendName}`}
        onclick={send}
      >
        {#if sendTo.icon}
          <img class="appicon" src={sendTo.icon} alt="" />
        {:else}
          <Icon name="send" />
        {/if}
        <span class="sendlabel">Send to {sendName}</span>
      </button>
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

  /* The window has no frame: the header (and the room beside the tabs) is
     its drag region. */
  .hdr {
    flex: 0 0 40px;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 0 6px 0 8px;
    user-select: none;
    -webkit-user-select: none;
  }
  .tabs {
    flex: 0 1 auto;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 2px;
    overflow-x: auto;
    scrollbar-width: none;
  }
  /* The rest of the header: room to drag the window by. */
  .grow {
    flex: 1;
    align-self: stretch;
  }
  .tab {
    flex: 0 1 auto;
    min-width: 52px;
    max-width: 150px;
    display: flex;
    align-items: center;
    height: 30px;
    border-radius: 8px;
    transition: background-color var(--yap-dur);
  }
  .tab:hover {
    background: var(--yap-paper-s2);
  }
  /* The tab in front keeps room for its name; the others give way. */
  .tab.on {
    flex-shrink: 0;
    min-width: 96px;
    max-width: 160px;
    background: var(--yap-paper-s2);
  }
  .tablabel {
    flex: 1;
    min-width: 0;
    height: 100%;
    padding: 0 2px 0 10px;
    border: none;
    background: transparent;
    color: var(--yap-paper-ink);
    text-align: left;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    font-weight: 650;
    font-size: 13px;
  }
  .tablabel.muted {
    color: var(--yap-paper-muted);
    font-weight: 550;
  }
  .tab:not(.on) .tablabel {
    font-weight: 550;
  }
  .tabx {
    flex: 0 0 auto;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    margin-right: 4px;
    padding: 0;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: var(--yap-paper-muted);
    opacity: 0;
    transition: opacity var(--yap-dur), background-color var(--yap-dur);
  }
  .tabx :global(svg) {
    width: 12px;
    height: 12px;
  }
  .tab.on .tabx,
  .tab:hover .tabx,
  .tabx:focus-visible {
    opacity: 1;
  }
  .tabx:hover {
    background: var(--yap-paper-s2-strong);
    color: var(--yap-paper-ink);
  }
  .tools {
    display: flex;
    align-items: center;
    gap: 2px;
  }
  .iconbtn {
    flex: 0 0 auto;
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
  .tablabel:focus-visible,
  .tabx:focus-visible,
  .mic:focus-visible,
  .retry:focus-visible,
  .send:focus-visible,
  .offerbtn:focus-visible,
  .offerx:focus-visible {
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

  /* On an empty page: a quick-note button on the Yap bar (bar_quick_note). */
  .offer {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 12px 10px;
    padding: 6px 6px 6px 12px;
    border-radius: 10px;
    background: var(--yap-paper-s2);
    font-size: 12.5px;
    user-select: none;
    -webkit-user-select: none;
  }
  .offertext {
    flex: 1;
    min-width: 0;
    color: var(--yap-paper-muted);
  }
  .offerbtn {
    flex: 0 0 auto;
    height: 26px;
    padding: 0 10px;
    border: none;
    border-radius: 7px;
    background: var(--yap-paper-ink);
    color: var(--yap-paper);
    font-size: 12.5px;
    font-weight: 600;
  }
  .offerx {
    flex: 0 0 auto;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    padding: 0;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: var(--yap-paper-muted);
  }
  .offerx :global(svg) {
    width: 12px;
    height: 12px;
  }
  .offerx:hover {
    background: var(--yap-paper-s2-strong);
  }

  .foot {
    flex: 0 0 40px;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 8px 0 12px;
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
  /* Send to {app}: the app's own icon and name. */
  .send {
    flex: 0 1 auto;
    min-width: 0;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding: 0 10px 0 8px;
    border: none;
    border-radius: 8px;
    background: var(--yap-paper-ink);
    color: var(--yap-paper);
    font-size: 12.5px;
    font-weight: 600;
    transition: opacity var(--yap-dur);
  }
  .send:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .send :global(svg) {
    width: 14px;
    height: 14px;
    flex: 0 0 auto;
  }
  .appicon {
    width: 16px;
    height: 16px;
    flex: 0 0 auto;
    border-radius: 3px;
  }
  .sendlabel {
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
</style>
