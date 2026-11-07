<script>
  // A Markdown file's window (`doc-…`, notes/md_file.rs, 2026-11-07): a `.md`
  // or `.markdown` file opened from Explorer ("Open with"), `yap.exe <path>`
  // or Settings → General's "Open a Markdown file…", one window per file.
  //
  // - **Preview**: the file as a page you can format (components/richtext/
  //   RichNote, the editor notes and Chat's documents use: a change rewrites
  //   only its own block, so the file stays its Markdown exactly), Ctrl+F in
  //   it. **Source**: its Markdown, coloured (core/markdownSource.js, a copy
  //   under a clear text box, as Chat's document panel has it) and editable,
  //   Ctrl+F too. The choice is remembered (`yap.doc.view`).
  // - **Saved back to the file** 600 ms after the last change (and on the
  //   way out: blur, close, Ctrl+S), one save at a time, each only while the
  //   file is still what the page last read or wrote (`expect`, its stamp:
  //   modified time and size); Rust keeps its encoding and line endings.
  // - **Changes on disk** (a job or an assistant writing the file): looked
  //   for every 1.5 s. Nothing typed and unsaved here → reloaded in place,
  //   where it was scrolled to; typed and unsaved → a quiet bar, "Changed on
  //   disk · Reload · Keep mine". Moved or deleted → "Save it here again".
  // - **The title bar**: the file's name and folder, Preview / Source, Read
  //   aloud (voice/read_aloud.rs, the reading card on the Yap bar), Copy (the
  //   Markdown), Open folder (Explorer, the file selected), and the window's
  //   own buttons (it has no frame). Closing saves first; unsaved changes
  //   that can't be (changed on disk, a failed save) are asked about.
  // - **Plain words when it can't open**: over 5 MB, gone, not allowed.
  //   Read-only (a read-only file, or text Yap can't write back): shown,
  //   not editable.
  // - The in-page hotkey fallback (a focused Yap window never sees the
  //   global hook): dictation, the meeting shortcut, the Voice Assistant,
  //   Ask about the selection, Read aloud, Snip, Copy last, Quick note, the
  //   language shortcuts.
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { onMount, tick } from 'svelte';
  import Icon from '../notepad/NotepadIcon.svelte';
  import ToastHost from '../ui/ToastHost.svelte';
  import { toast } from '../ui/toast.svelte.js';
  import SummaryFind from '../components/SummaryFind.svelte';
  import { richtext, loadRichtext } from '../components/richtext/richtext.svelte.js';
  import { highlightMarkdown } from '../core/markdownSource.js';
  import { actionMatchesKeydown, actionMatchesKeyup, hotkeyMatchesKeydown, hotkeyMatchesKeyup } from '../core/hotkeys.js';
  import { languageShortcuts } from '../settings/languages.svelte.js';
  import { askSelectionKey, initSpeech, readAloud, readPageSelection, speech } from '../stores/speech.svelte.js';
  import { askConfirm, menuOrDialogOpen } from '../ui/contextMenu.svelte.js';
  import { afterRead, statusLine, formLine, POLL_MS, SAVE_MS } from './docFile.js';

  const appWindow = getCurrentWindow();
  const LABEL = appWindow.label;
  /** Read aloud's id for this file (stores/speech.svelte.js). */
  const SPEECH_ID = `doc:${LABEL}`;
  const VIEW_KEY = 'yap.doc.view';

  // The file: `{ path, name, folder }`.
  let info = $state(null);
  // The last read: `{ encoding, eol, readOnly, size }`.
  let loaded = $state(null);
  // What's on the page now (its Markdown, `\n` line breaks).
  let text = $state('');
  // Couldn't open it (too big, gone, not allowed): what to say.
  let error = $state('');
  let ready = $state(false);
  let view = $state(readView());
  // '' | 'saving' | 'saved' | 'unsaved' | 'readonly'
  let saveState = $state('');
  let saveError = $state('');
  // The file on disk: null (in step) | 'changed' (there and here) | 'gone'.
  let disk = $state(null);
  let maximized = $state(false);
  let copied = $state(false);
  let cfg = $state(null);

  let ed = $state.raw(null);
  let pageEl = $state(null);
  let srcScroll = $state(null);
  let srcBox = $state(null);
  let mirrorEl = $state(null);
  let finder = $state(null);
  let finding = $state(false);

  // The file as the page last read or wrote it, and its stamp then.
  let base = '';
  let stamp = null;
  // The file's newer words while `disk` is 'changed' (Reload takes them).
  let theirs = null;
  // Typed since the last save went out.
  let dirty = false;
  let timer = null;
  // The saves, one after another.
  let chain = Promise.resolve();
  // Reads and saves in flight: the look at the file waits for them.
  let busy = 0;
  let closing = false;

  const readOnly = $derived(loaded?.readOnly || '');
  const speaking = $derived(speech.speakingId === SPEECH_ID);
  const bar = $derived(
    disk === 'changed'
      ? { kind: 'changed', text: 'Changed on disk' }
      : disk === 'gone'
        ? { kind: 'gone', text: 'Moved or deleted on disk' }
        : saveError
          ? { kind: 'error', text: saveError }
          : readOnly
            ? { kind: 'readonly', text: readOnly }
            : null
  );

  function readView() {
    try {
      return localStorage.getItem(VIEW_KEY) === 'source' ? 'source' : 'preview';
    } catch {
      return 'preview';
    }
  }

  // ---- reading -------------------------------------------------------------------------

  /** The file's words in place, as read (`r`): nothing unsaved any more. */
  function take(r) {
    loaded = r;
    text = r.text;
    base = r.text;
    stamp = r.stamp;
    theirs = null;
    disk = null;
    dirty = false;
    clearTimeout(timer);
    timer = null;
    saveError = '';
    saveState = r.readOnly ? 'readonly' : '';
  }

  async function load() {
    busy++;
    try {
      const r = await invoke('md_file_read');
      error = '';
      take(r);
      ready = true;
    } catch (e) {
      error = String(e);
      ready = false;
    } finally {
      busy--;
    }
  }

  /** What `fn` changes, kept where it was scrolled to. */
  async function keepScroll(fn) {
    const el = view === 'source' ? srcScroll : document.querySelector('.richscroll');
    const top = el?.scrollTop ?? 0;
    fn();
    await tick();
    requestAnimationFrame(() => {
      const now = view === 'source' ? srcScroll : document.querySelector('.richscroll');
      if (now) now.scrollTop = top;
    });
  }

  /** The file read again (it changed on disk, or a save found it had). */
  async function readAgain() {
    busy++;
    try {
      let r;
      try {
        r = await invoke('md_file_read');
      } catch (e) {
        const now = await invoke('md_file_stat').catch(() => null);
        if (now == null) disk = 'gone';
        else saveError = String(e);
        return;
      }
      const what = afterRead({ text, base, dirty: dirty || timer !== null }, r.text);
      if (what === 'reload') {
        await keepScroll(() => take(r));
        return;
      }
      loaded = r;
      stamp = r.stamp;
      if (what === 'same') {
        base = r.text;
        theirs = null;
        disk = null;
        dirty = false;
        clearTimeout(timer);
        timer = null;
        saveState = 'saved';
      } else if (what === 'touched') {
        disk = null;
        dirty = true;
        flush();
      } else {
        theirs = r;
        disk = 'changed';
        saveState = 'unsaved';
      }
    } finally {
      busy--;
    }
  }

  /** Every 1.5 s: has the file changed on disk? */
  async function look() {
    if (busy || !ready || closing) return;
    let now;
    try {
      now = await invoke('md_file_stat');
    } catch {
      return;
    }
    // A save or a read started meanwhile: it has the stamp.
    if (busy || now === stamp) return;
    if (now == null) {
      disk = disk === 'changed' && theirs ? disk : 'gone';
      return;
    }
    if (disk === 'gone') disk = null;
    await readAgain();
  }

  // ---- saving --------------------------------------------------------------------------

  function onChange(md) {
    if (readOnly || md === text) return;
    text = md;
    dirty = true;
    saveState = 'saving';
    clearTimeout(timer);
    timer = setTimeout(flush, SAVE_MS);
  }

  /** Save what's typed now (after any save on its way); resolves once it's saved. */
  function flush() {
    clearTimeout(timer);
    timer = null;
    if (!dirty || readOnly || disk === 'changed') return chain;
    dirty = false;
    const content = text;
    chain = chain.then(() => saveNow(content));
    return chain;
  }

  async function saveNow(content, { force = false } = {}) {
    busy++;
    try {
      saveState = 'saving';
      const s = await invoke('md_file_save', { text: content, expect: force ? null : stamp });
      if (s?.conflict) {
        dirty = true;
        if (!s.stamp) {
          disk = 'gone';
          saveState = 'unsaved';
        } else {
          await readAgain();
        }
        return;
      }
      stamp = s?.stamp ?? stamp;
      base = content;
      disk = null;
      theirs = null;
      saveError = '';
      if (!dirty && timer === null) saveState = 'saved';
    } catch (e) {
      dirty = true;
      saveError = String(e);
      saveState = 'unsaved';
    } finally {
      busy--;
    }
  }

  /** Changed on disk: the file's words here, what was typed let go. */
  async function reload() {
    if (theirs) {
      const r = theirs;
      await keepScroll(() => take(r));
    } else {
      await load();
    }
  }

  /** Changed on disk, or gone: what's here, written over it (or back). */
  function keepMine() {
    disk = null;
    theirs = null;
    saveError = '';
    clearTimeout(timer);
    timer = null;
    dirty = false;
    const content = text;
    chain = chain.then(() => saveNow(content, { force: true }));
    return chain;
  }

  /** A failed save, tried again. */
  function tryAgain() {
    saveError = '';
    dirty = true;
    return flush();
  }

  // ---- the title bar -------------------------------------------------------------------

  async function setView(v) {
    if (view === v) return;
    finder?.close?.();
    finding = false;
    view = v;
    try {
      localStorage.setItem(VIEW_KEY, v);
    } catch {
      /* this time only */
    }
    await tick();
    if (v === 'source') srcBox?.focus({ preventScroll: true });
    else ed?.focus?.();
  }

  function viewKey(e) {
    if (e.key === 'ArrowRight' || e.key === 'ArrowLeft') {
      e.preventDefault();
      const next = view === 'source' ? 'preview' : 'source';
      setView(next);
      e.currentTarget.parentElement?.querySelector(`[data-view="${next}"]`)?.focus();
    }
  }

  function readIt() {
    readAloud(text, SPEECH_ID);
  }

  async function copy() {
    try {
      await navigator.clipboard.writeText(text);
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch {
      toast({ title: "Couldn't copy", description: 'The clipboard is busy. Try again.', variant: 'destructive' });
    }
  }

  function openFolder() {
    invoke('md_file_reveal').catch((e) => toast({ title: "Couldn't open the folder", description: String(e), variant: 'destructive' }));
  }

  /** Close: what's typed saved first; changes that can't be are asked about. */
  async function close() {
    if (closing) return;
    closing = true;
    await flush();
    if (dirty || disk === 'changed' || saveError) {
      closing = false;
      const why = disk === 'changed' ? ': the file changed on disk since' : disk === 'gone' ? ': the file is gone from its folder' : '';
      askConfirm({
        title: 'Close without saving?',
        body: `Your changes to ${info?.name || 'this file'} aren't saved${why}.`,
        confirmLabel: 'Close anyway',
        busyLabel: 'Closing…',
        danger: true,
        run: () => invoke('md_file_close').then(() => true, () => false),
      });
      return;
    }
    invoke('md_file_close').catch(() => (closing = false));
  }

  async function refreshMaximized() {
    try {
      maximized = await appWindow.isMaximized();
    } catch {
      /* keep */
    }
  }

  // ---- Source's box follows the file (a reload) while you're not typing: the caret stays ----

  $effect(() => {
    const t = text;
    const el = srcBox;
    if (!el || el.value === t) return;
    const focused = document.activeElement === el;
    const start = el.selectionStart ?? 0;
    const before = el.value.slice(0, start);
    const caret = t.startsWith(before) ? start : Math.max(0, start + (t.length - el.value.length));
    el.value = t;
    if (focused) el.setSelectionRange(caret, caret);
  });

  // ---- keys: the file's own, then the in-page hotkey fallback (the WebView2-focus gotcha) ----

  /** Ctrl+F: find in the view showing. */
  function find(e) {
    if (view === 'source') {
      finder?.show();
      return;
    }
    // The page's own Ctrl+F (RichNote) takes it there.
    if (pageEl && !pageEl.contains(e.target)) pageEl.dispatchEvent(new KeyboardEvent('keydown', { key: 'f', ctrlKey: true, bubbles: true }));
  }

  function onKeyDown(e) {
    if (e.isComposing) return;
    const ctrl = (e.ctrlKey || e.metaKey) && !e.altKey && !e.shiftKey;
    if (ctrl && ['s', 'w', 'f'].includes(e.key.toLowerCase()) && ready) {
      const k = e.key.toLowerCase();
      if (k === 'f') {
        if (view === 'source' || !pageEl?.contains(e.target)) {
          e.preventDefault();
          if (!e.repeat) find(e);
        }
        return;
      }
      e.preventDefault();
      if (e.repeat) return;
      if (k === 's') flush();
      else if (!menuOrDialogOpen()) close();
      return;
    }
    if (e.repeat || !cfg) return;
    const lang = languageShortcuts(cfg).find((s) => hotkeyMatchesKeydown(e, s.hotkey));
    if (actionMatchesKeydown(e, cfg, 'meetingHotkey')) {
      e.preventDefault();
      e.stopPropagation();
      flush();
      invoke('meeting_shortcut', { origin: LABEL }).catch(() => {});
    } else if (actionMatchesKeydown(e, cfg, 'assistantHotkey')) {
      e.preventDefault();
      e.stopPropagation();
      invoke('assistant_key', { pressed: true }).catch(() => {});
    } else if (actionMatchesKeydown(e, cfg, 'askHotkey')) {
      // Ask about the selection (chat/assistant/selected.rs): what's selected here.
      e.preventDefault();
      e.stopPropagation();
      askSelectionKey(true);
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
    } else if (actionMatchesKeydown(e, cfg, 'quickNoteHotkey')) {
      // The quick note (notes/quick_note/key.rs) comes up in front; held, a dictation into it.
      e.preventDefault();
      e.stopPropagation();
      flush();
      invoke('quick_note_key', { pressed: true }).catch(() => {});
    } else if (lang) {
      e.preventDefault();
      e.stopPropagation();
      invoke('language_key', { id: lang.id, pressed: true }).catch(() => {});
    } else if (actionMatchesKeydown(e, cfg, 'hotkey')) {
      // Dictation: pasted at the caret, as into any text box.
      e.preventDefault();
      e.stopPropagation();
      invoke('dictation_key', { pressed: true }).catch(() => {});
    }
  }

  // The key coming up: push-to-talk's release (Rust decides by the mode).
  function onKeyUp(e) {
    if (!cfg) return;
    const lang = languageShortcuts(cfg).find((s) => hotkeyMatchesKeyup(e, s.hotkey));
    if (actionMatchesKeyup(e, cfg, 'quickNoteHotkey')) {
      e.preventDefault();
      invoke('quick_note_key', { pressed: false }).catch(() => {});
    }
    if (actionMatchesKeyup(e, cfg, 'assistantHotkey')) {
      e.preventDefault();
      invoke('assistant_key', { pressed: false }).catch(() => {});
    } else if (actionMatchesKeyup(e, cfg, 'askHotkey')) {
      e.preventDefault();
      askSelectionKey(false);
    } else if (lang) {
      e.preventDefault();
      invoke('language_key', { id: lang.id, pressed: false }).catch(() => {});
    } else if (actionMatchesKeyup(e, cfg, 'hotkey')) {
      e.preventDefault();
      invoke('dictation_key', { pressed: false }).catch(() => {});
    }
  }

  async function refreshConfig() {
    try {
      cfg = await invoke('get_config');
    } catch {
      /* keep */
    }
  }

  onMount(() => {
    initSpeech(['doc:']);
    loadRichtext();
    refreshConfig();
    refreshMaximized();
    invoke('md_file_info')
      .then((i) => (info = i))
      .catch((e) => (error = String(e)))
      .then(() => (info ? load() : null));
    window.addEventListener('keydown', onKeyDown, true);
    window.addEventListener('keyup', onKeyUp, true);
    // Saved on the way out of the window (another app, the taskbar's close).
    const away = () => flush();
    window.addEventListener('blur', away);
    const poll = setInterval(look, POLL_MS);
    const uns = [];
    const on = (name, fn) => listen(name, (e) => fn(e.payload)).then((u) => uns.push(u));
    // The window's close (Alt+F4, the taskbar): Rust asks the page first.
    on('yap-doc-closing', () => close());
    on('yap-config-saved', refreshConfig);
    let gone = false;
    let unResized = null;
    appWindow.onResized(() => refreshMaximized()).then((u) => (gone ? u() : (unResized = u)));
    return () => {
      gone = true;
      flush();
      clearInterval(poll);
      unResized?.();
      window.removeEventListener('keydown', onKeyDown, true);
      window.removeEventListener('keyup', onKeyUp, true);
      window.removeEventListener('blur', away);
      uns.forEach((u) => u());
    };
  });
</script>

<main class="doc" aria-label={info?.name || 'Markdown file'}>
  <!-- The window has no frame: the title bar is its drag region. -->
  <header class="hdr" data-tauri-drag-region>
    <span class="fileicon" aria-hidden="true" data-tauri-drag-region><Icon name="markdown" /></span>
    <div class="names" title={info?.path ?? ''} data-tauri-drag-region>
      <h1 class="name" data-tauri-drag-region>{info?.name ?? ''}</h1>
      <!-- Cut at its start when long: the folders nearest the file show. -->
      <p class="folder" data-tauri-drag-region><span dir="ltr" data-tauri-drag-region>{info?.folder ?? ''}</span></p>
    </div>
    <span class="grow" data-tauri-drag-region></span>
    {#if ready}
      <!-- Preview (the page) or Source (its Markdown); ←/→ switch. -->
      <div class="viewseg" role="group" aria-label="View">
        {#each [['preview', 'Preview', 'The file as a page'], ['source', 'Source', 'Its Markdown, to read or edit']] as [v, name, hint] (v)}
          {@const on = view === v}
          <button type="button" class="segbtn" class:on data-view={v} aria-pressed={on} tabindex={on ? 0 : -1} title={hint} onclick={() => setView(v)} onkeydown={viewKey}
            >{name}</button
          >
        {/each}
      </div>
      <button
        class="iconbtn"
        aria-label={speaking ? 'Stop reading aloud' : 'Read aloud'}
        aria-pressed={speaking}
        title={speaking ? 'Stop reading aloud' : 'Read the file aloud'}
        onclick={readIt}
      >
        <Icon name={speaking ? 'stop' : 'speaker'} />
      </button>
      <button class="iconbtn" aria-label="Copy" title={copied ? 'Copied' : 'Copy the Markdown'} onclick={copy}>
        <Icon name={copied ? 'check' : 'copy'} />
      </button>
    {/if}
    <button class="iconbtn" aria-label="Open folder" title="Show the file in its folder" onclick={openFolder} disabled={!info}>
      <Icon name="folder" />
    </button>
    <div class="winbtns">
      <button class="winbtn" aria-label="Minimize" onclick={() => appWindow.minimize()}><Icon name="min" /></button>
      <button class="winbtn max" aria-label={maximized ? 'Restore' : 'Maximize'} onclick={() => appWindow.toggleMaximize()}>
        <Icon name={maximized ? 'restore' : 'max'} />
      </button>
      <button class="winbtn close" aria-label="Close" onclick={close}><Icon name="close" /></button>
    </div>
  </header>

  {#if bar && ready}
    <!-- What's up with the file: changed on disk, gone, a failed save, read-only. -->
    <div class="bar" class:quiet={bar.kind === 'readonly'} role={bar.kind === 'error' ? 'alert' : 'status'} data-testid="doc-bar">
      {#if bar.kind !== 'readonly'}<Icon name="warn" />{/if}
      <span class="bartext">{bar.text}</span>
      {#if bar.kind === 'changed'}
        <button class="barbtn" onclick={reload}>Reload</button>
        <button class="barbtn" onclick={keepMine}>Keep mine</button>
      {:else if bar.kind === 'gone'}
        <button class="barbtn" onclick={keepMine}>Save it here again</button>
      {:else if bar.kind === 'error'}
        <button class="barbtn" onclick={tryAgain}>Try again</button>
      {/if}
    </div>
  {/if}

  {#if error}
    <div class="failed" role="alert">
      <p class="failtext">{error}</p>
      <div class="failbtns">
        <button class="barbtn" onclick={load} disabled={!info}>Try again</button>
        <button class="barbtn" onclick={openFolder} disabled={!info}>Open folder</button>
      </div>
    </div>
  {:else if !ready}
    <div class="loading" aria-busy="true"></div>
  {:else if view === 'source'}
    <div class="srcfind">
      <SummaryFind bind:this={finder} bind:open={finding} root={() => mirrorEl} {text} look="paper" label="Find in the file" />
    </div>
    <!-- Its Markdown, coloured: a copy under a clear text box, character for character. -->
    <div class="srcscroll" bind:this={srcScroll}>
      <div class="srcwrap">
        <!-- eslint-disable-next-line svelte/no-at-html-tags — highlightMarkdown escapes every character it doesn't wrap -->
        <pre class="srcmirror" aria-hidden="true" bind:this={mirrorEl}>{@html highlightMarkdown(text)}{'\n​'}</pre>
        <textarea
          class="srcedit"
          bind:this={srcBox}
          aria-label="Markdown source"
          spellcheck="false"
          readonly={!!readOnly}
          oninput={(e) => onChange(e.currentTarget.value)}
        ></textarea>
      </div>
    </div>
  {:else if richtext.Note}
    {@const Rich = richtext.Note}
    <Rich
      {text}
      onchange={onChange}
      label={info?.name || 'Markdown file'}
      readonly={!!readOnly}
      look="paper"
      toolbarLabel="Format the file"
      rawTitle="Edit this part in Source"
      bind:el={pageEl}
      bind:ed
      --rich-size="calc(15px * var(--yap-reading-scale))"
      --rich-leading="calc(1.6 * var(--yap-reading-leading))"
      --rich-pad="14px max(28px, calc((100% - 760px) / 2)) 48px"
    />
  {:else}
    <!-- Until the page has loaded: the file in a plain box. -->
    <textarea class="plain" data-rich-fallback value={text} readonly={!!readOnly} aria-label={info?.name || 'Markdown file'} spellcheck="true" oninput={(e) => onChange(e.currentTarget.value)}
    ></textarea>
  {/if}

  <footer class="foot">
    <span class="status" class:warn={saveState === 'unsaved'} role="status" data-testid="doc-status">{statusLine(saveState)}</span>
    <span class="grow"></span>
    {#if ready}<span class="form">{formLine(loaded)}</span>{/if}
  </footer>
</main>

<ToastHost />

<style>
  button {
    font: inherit;
    cursor: pointer;
  }
  .doc {
    display: flex;
    flex-direction: column;
    height: 100vh;
    background: var(--yap-paper);
    color: var(--yap-paper-ink);
    font-size: 14px;
  }

  .hdr {
    flex: 0 0 48px;
    display: flex;
    align-items: center;
    gap: 4px;
    min-width: 0;
    padding: 0 0 0 12px;
    user-select: none;
    -webkit-user-select: none;
  }
  .fileicon {
    flex: 0 0 auto;
    display: inline-flex;
    color: var(--yap-paper-muted);
  }
  .fileicon :global(svg) {
    width: 18px;
    height: 18px;
  }
  .names {
    flex: 0 1 auto;
    min-width: 0;
    display: flex;
    flex-direction: column;
    padding-left: 6px;
  }
  .name {
    margin: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    font-size: 13.5px;
    font-weight: 650;
    line-height: 18px;
  }
  .folder {
    margin: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    /* The ellipsis at the start: "…\notes\plans" (the text itself stays left to right). */
    direction: rtl;
    text-align: left;
    color: var(--yap-paper-muted);
    font-size: 11.5px;
    line-height: 15px;
  }
  .grow {
    flex: 1 1 auto;
    align-self: stretch;
    min-width: 12px;
  }
  .viewseg {
    flex: 0 0 auto;
    display: inline-flex;
    margin: 0 4px;
    padding: 2px;
    border-radius: 8px;
    background: var(--yap-paper-s1);
  }
  .segbtn {
    height: 24px;
    padding: 0 9px;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--yap-paper-muted);
    font-size: 11.5px;
    font-weight: 600;
  }
  .segbtn:hover {
    color: var(--yap-paper-ink);
  }
  .segbtn.on {
    background: var(--yap-paper-card);
    color: var(--yap-paper-ink);
    box-shadow: 0 1px 2px color-mix(in srgb, var(--yap-paper-ink) 12%, transparent);
  }
  .iconbtn {
    flex: 0 0 auto;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
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
  .winbtns {
    flex: 0 0 auto;
    display: flex;
    margin-left: 6px;
  }
  .winbtn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 44px;
    height: 48px;
    padding: 0;
    border: none;
    background: transparent;
    color: var(--yap-paper-ink);
    transition: background-color var(--yap-dur);
  }
  .winbtn :global(svg) {
    width: 14px;
    height: 14px;
  }
  .winbtn.max :global(svg) {
    width: 13px;
    height: 13px;
  }
  .winbtn.close :global(svg) {
    width: 16px;
    height: 16px;
  }
  .winbtn:hover {
    background: var(--yap-paper-s2);
  }
  /* Windows' own close red, in both themes. */
  .winbtn.close:hover {
    background: #c42b1c;
    color: #fff;
  }
  .iconbtn:focus-visible,
  .segbtn:focus-visible,
  .winbtn:focus-visible,
  .barbtn:focus-visible {
    outline: none;
    box-shadow: 0 0 0 3px var(--yap-paper-ring);
  }

  /* Changed on disk · Reload · Keep mine (and the other notes about the file). */
  .bar {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 12px 6px;
    padding: 5px 6px 5px 10px;
    border-radius: 10px;
    background: var(--yap-paper-s2);
    font-size: 12.5px;
    user-select: none;
    -webkit-user-select: none;
  }
  .bar.quiet {
    background: var(--yap-paper-s1);
    padding: 7px 10px;
  }
  .bar :global(svg) {
    flex: 0 0 auto;
    width: 14px;
    height: 14px;
    color: var(--yap-paper-snag);
  }
  .bartext {
    flex: 1 1 auto;
    min-width: 0;
    color: var(--yap-paper-text-2);
  }
  .barbtn {
    flex: 0 0 auto;
    height: 26px;
    padding: 0 10px;
    border: none;
    border-radius: 7px;
    background: var(--yap-paper-card);
    color: var(--yap-paper-ink);
    font-size: 12.5px;
    font-weight: 600;
    box-shadow: 0 1px 2px color-mix(in srgb, var(--yap-paper-ink) 10%, transparent);
  }
  .barbtn:hover:not(:disabled) {
    background: var(--yap-paper-s2-strong);
  }
  .barbtn:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .failed {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 14px;
    padding: 24px;
    text-align: center;
  }
  .failtext {
    max-width: 440px;
    margin: 0;
    color: var(--yap-paper-text-2);
    font-size: 14px;
    line-height: 1.5;
  }
  .failbtns {
    display: flex;
    gap: 8px;
  }
  .loading {
    flex: 1 1 auto;
  }

  /* Until the page has loaded: the plain box, the reading text. */
  .plain {
    flex: 1 1 auto;
    min-height: 0;
    margin: 0;
    padding: 14px 28px 48px;
    border: none;
    background: transparent;
    color: var(--yap-paper-ink);
    font-family: var(--yap-font-reading);
    font-size: calc(15px * var(--yap-reading-scale));
    line-height: calc(1.6 * var(--yap-reading-leading));
    resize: none;
    outline: none;
  }

  /* **Source**: the Markdown, coloured, under a clear text box that takes
     the typing (after Chat's document panel): the two share every measure,
     so the caret and the selection sit on the coloured words. */
  .srcfind :global(.find) {
    position: static;
    margin: 4px 12px 0;
  }
  .srcscroll {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
  }
  .srcscroll::-webkit-scrollbar {
    width: 8px;
  }
  .srcscroll::-webkit-scrollbar-thumb {
    border-radius: 4px;
    background: color-mix(in srgb, var(--yap-paper-ink) 16%, transparent);
  }
  .srcwrap {
    position: relative;
    min-height: 100%;
  }
  .srcmirror,
  .srcedit {
    box-sizing: border-box;
    margin: 0;
    padding: 14px 28px 48px;
    border: none;
    font-family: ui-monospace, Consolas, monospace;
    font-size: calc(13px * var(--yap-reading-scale));
    line-height: 1.6;
    letter-spacing: normal;
    tab-size: 4;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    word-break: normal;
  }
  .srcmirror {
    min-height: 100%;
    color: var(--yap-paper-ink);
    pointer-events: none;
  }
  .srcedit {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    resize: none;
    overflow: hidden;
    background: transparent;
    color: transparent;
    -webkit-text-fill-color: transparent;
    caret-color: var(--yap-paper-ink);
    outline: none;
  }
  .srcedit::selection {
    background: color-mix(in srgb, var(--yap-primary) 26%, transparent);
  }
  /* Colour only — never a bolder or slanted face, which could move a glyph
     out from under the text box's caret. */
  .srcmirror :global(.md-s-heading) {
    color: var(--yap-primary);
  }
  .srcmirror :global(.md-s-mark) {
    color: var(--yap-paper-muted);
  }
  .srcmirror :global(.md-s-strong) {
    color: var(--yap-paper-ink);
    text-decoration: underline 1px color-mix(in srgb, var(--yap-paper-ink) 25%, transparent);
    text-underline-offset: 3px;
  }
  .srcmirror :global(.md-s-em) {
    color: var(--yap-paper-text-2);
  }
  .srcmirror :global(.md-s-link) {
    color: var(--yap-link);
  }
  .srcmirror :global(.md-s-url) {
    color: var(--yap-paper-muted);
  }
  .srcmirror :global(.md-s-icode),
  .srcmirror :global(.md-s-code) {
    color: var(--yap-paper-text-2);
    background: var(--yap-paper-s1);
  }
  .srcmirror :global(.md-s-fence) {
    color: var(--yap-paper-muted);
  }
  .srcmirror :global(.md-s-quote) {
    color: var(--yap-paper-text-2);
  }
  .srcmirror :global(.md-s-comment) {
    color: var(--yap-paper-muted);
    background: var(--yap-paper-s1);
  }

  .foot {
    flex: 0 0 28px;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 14px;
    border-top: 1px solid var(--yap-paper-s2);
    color: var(--yap-paper-muted);
    font-size: 11.5px;
    user-select: none;
    -webkit-user-select: none;
  }
  .status.warn {
    color: var(--yap-paper-snag);
  }
</style>
