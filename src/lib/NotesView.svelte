<script>
  // Notes — the AI Notepad surface, ported from OpenWhispr's
  // PersonalNotesView.tsx core loop: a notes list + a markdown editor with a
  // Raw ↔ Enhanced dual view. "Enhance" runs the Actions engine
  // (note_enhance → llm::NOTE_BASE_PROMPT + the Note Formatting scope's
  // editable fragment, temp 0.3) writing to enhanced_content — the raw note is
  // never overwritten. A staleness dot appears when the raw content changes
  // after an enhancement (OpenWhispr's len+first-50 hash).
  //
  // v1 scope (per ROADMAP "AI Notepad"): plain-textarea markdown editing +
  // safe rendered Enhanced view (lib/markdown.js) — a rich editor
  // (Milkdown/CodeMirror) and folders come later; meeting notes arrive with
  // the Phase-6 recorder.
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { renderMarkdown } from './markdown.js';
  import ActionManager from './ActionManager.svelte';

  let notes = $state([]); // list summaries (all folders)
  let folders = $state(['Personal', 'Meetings']);
  let activeFolder = $state('Personal');
  let selected = $state(null); // full note object
  let tab = $state('raw'); // raw | enhanced
  let enhancing = $state(new Set()); // note ids with an enhancement in flight
  let error = $state(null);
  let copied = $state(false);
  let saveTimer = null;
  // Search notes (the OpenWhispr sidebar action): filters the list pane.
  let searchOpen = $state(false);
  let query = $state('');
  let searchEl = $state(null);
  // Inline "new folder" input (the + next to FOLDERS).
  let addingFolder = $state(false);
  let newFolderName = $state('');
  // Actions (named prompt fragments) — the picker + manager dialog.
  let actions = $state([]);
  let actionsOpen = $state(false); // the manager dialog
  let pickerOpen = $state(false); // the split-button dropdown
  let lastUsedActionId = $state(
    Number(localStorage.getItem('yapLastUsedActionId')) || null
  );
  const activeAction = $derived(
    actions.find((a) => a.id === lastUsedActionId) ?? actions[0] ?? null
  );

  async function refreshActions() {
    try {
      actions = (await invoke('notes_actions')) || [];
    } catch {
      actions = [];
    }
  }

  async function refreshList() {
    try {
      notes = (await invoke('notes_list')) || [];
    } catch {
      notes = [];
    }
    try {
      folders = (await invoke('notes_folders')) || folders;
    } catch {
      /* keep seeded defaults */
    }
  }

  // Notes shown in the list pane: the active folder, filtered by search.
  const shownNotes = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return notes.filter(
      (n) =>
        (n.folder || 'Personal') === activeFolder &&
        (!q ||
          (n.title || '').toLowerCase().includes(q) ||
          (n.preview || '').toLowerCase().includes(q))
    );
  });

  function toggleSearch() {
    searchOpen = !searchOpen;
    if (searchOpen) {
      setTimeout(() => searchEl?.focus(), 0);
    } else {
      query = '';
    }
  }

  async function createFolder() {
    const name = newFolderName.trim();
    addingFolder = false;
    newFolderName = '';
    if (!name) return;
    try {
      folders = await invoke('notes_folder_create', { name });
      activeFolder = name;
    } catch {
      /* ignore */
    }
  }

  async function select(id) {
    flushSave();
    try {
      selected = await invoke('note_get', { id });
      tab = 'raw';
      error = null;
    } catch (e) {
      error = String(e);
    }
  }

  async function newNote() {
    flushSave();
    try {
      const note = await invoke('note_create', { folder: activeFolder });
      await refreshList();
      selected = note;
      tab = 'raw';
      error = null;
    } catch (e) {
      error = String(e);
    }
  }

  async function removeNote(id) {
    try {
      await invoke('note_delete', { id });
    } catch {
      /* ignore */
    }
    if (selected?.id === id) selected = null;
    refreshList();
  }

  // Debounced autosave of title + content while typing.
  function queueSave() {
    clearTimeout(saveTimer);
    saveTimer = setTimeout(flushSave, 600);
  }
  function flushSave() {
    clearTimeout(saveTimer);
    saveTimer = null;
    if (!selected) return;
    const { id, title, content } = selected;
    invoke('note_update', { id, title, content })
      .then(refreshList)
      .catch(() => {});
  }

  const isStale = $derived.by(() => {
    if (!selected || !selected.enhancedContent) return false;
    const s = notes.find((n) => n.id === selected.id);
    return !!s?.stale;
  });

  async function runAction(action) {
    if (!selected || enhancing.has(selected.id) || !action) return;
    pickerOpen = false;
    lastUsedActionId = action.id;
    localStorage.setItem('yapLastUsedActionId', String(action.id));
    flushSave();
    const id = selected.id;
    enhancing = new Set([...enhancing, id]);
    error = null;
    try {
      const enhanced = await invoke('note_enhance', { id, actionId: action.id });
      // Only mutate the open note if the user is still on it.
      if (selected?.id === id) {
        selected.enhancedContent = enhanced;
        tab = 'enhanced'; // OpenWhispr auto-switches to the Enhanced tab
      }
      refreshList();
    } catch (e) {
      if (selected?.id === id) error = String(e);
    } finally {
      const next = new Set(enhancing);
      next.delete(id);
      enhancing = next;
    }
  }

  async function copyEnhanced() {
    const text = tab === 'enhanced' ? selected?.enhancedContent : selected?.content;
    if (!text) return;
    try {
      await navigator.clipboard.writeText(text);
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch {
      /* clipboard unavailable */
    }
  }

  // Title fallback = first 6 words of content (OpenWhispr's fallback).
  function displayTitle(n) {
    if (n.title?.trim()) return n.title;
    const words = (n.preview || n.content || '').trim().split(/\s+/).filter(Boolean);
    if (!words.length) return 'Untitled note';
    return words.slice(0, 6).join(' ') + (words.length > 6 ? '…' : '');
  }

  function dateOf(ts) {
    const d = new Date(ts * 1000);
    const today = new Date();
    if (d.toDateString() === today.toDateString()) {
      return d.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' });
    }
    return d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
  }

  onMount(() => {
    refreshList();
    refreshActions();
    return () => flushSave();
  });
</script>

<svelte:window onclick={() => (pickerOpen = false)} />

<ActionManager bind:open={actionsOpen} onchanged={refreshActions} />

<div class="notes">
  <aside class="list">
    <!-- Action rows, OpenWhispr order: New note / Search notes -->
    <div class="tools">
      <button class="tool" onclick={newNote}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M13.4 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-7.4" /><path d="M18.4 2.6a2 2 0 0 1 2.8 2.8L13 13.6 9 14.6l1-4z" /></svg>
        New note
      </button>
      <button class="tool" class:on={searchOpen} onclick={toggleSearch}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="11" cy="11" r="7" /><path d="m21 21-4.3-4.3" /></svg>
        Search notes
      </button>
      <button class="tool" onclick={() => (actionsOpen = true)}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3v3M12 18v3M3 12h3M18 12h3M5.6 5.6l2.1 2.1M16.3 16.3l2.1 2.1M18.4 5.6l-2.1 2.1M7.7 16.3l-2.1 2.1" /></svg>
        Actions
      </button>
      {#if searchOpen}
        <input
          class="searchinput"
          bind:this={searchEl}
          bind:value={query}
          placeholder="Search…"
          onkeydown={(e) => e.key === 'Escape' && toggleSearch()}
        />
      {/if}
    </div>

    <!-- FOLDERS -->
    <div class="seccap">
      <span>Folders</span>
      <button class="secadd" title="New folder" aria-label="New folder" onclick={() => (addingFolder = true)}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
      </button>
    </div>
    <div class="folderlist">
      {#each folders as f (f)}
        <button class="folder" class:active={activeFolder === f} onclick={() => (activeFolder = f)}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 20h16a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.7-.9L9.2 3.9A2 2 0 0 0 7.5 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2z" /></svg>
          {f}
        </button>
      {/each}
      {#if addingFolder}
        <input
          class="searchinput"
          placeholder="Folder name…"
          bind:value={newFolderName}
          onkeydown={(e) => {
            if (e.key === 'Enter') createFolder();
            if (e.key === 'Escape') {
              addingFolder = false;
              newFolderName = '';
            }
          }}
          onblur={createFolder}
        />
      {/if}
    </div>

    <!-- NOTES -->
    <div class="seccap"><span>Notes</span></div>
    {#if shownNotes.length === 0}
      <div class="listempty">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" /><path d="M14 2v6h6M9 13h6M9 17h4" /></svg>
        <p>{query.trim() ? 'No matching notes' : 'No notes in this folder'}</p>
        {#if !query.trim()}
          <button class="createbtn" onclick={newNote}>+ Create note</button>
        {/if}
      </div>
    {:else}
      <div class="items">
        {#each shownNotes as n (n.id)}
          <div
            class="item"
            class:active={selected?.id === n.id}
            role="button"
            tabindex="0"
            onclick={() => select(n.id)}
            onkeydown={(e) => (e.key === 'Enter' || e.key === ' ') && select(n.id)}
          >
            <div class="itembody">
              <p class="ititle">
                {displayTitle(n)}
                {#if n.stale}<span class="staledot" title="Note changed since enhancement"></span>{/if}
              </p>
              <p class="imeta">
                {dateOf(n.updatedTs)}
                {#if n.hasEnhanced}
                  · <span class="enhtag">enhanced</span>
                {/if}
              </p>
            </div>
            <button
              class="idel"
              title="Delete note"
              aria-label="Delete note"
              onclick={(e) => {
                e.stopPropagation();
                removeNote(n.id);
              }}
            >
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 6h18M8 6V4a1 1 0 0 1 1-1h6a1 1 0 0 1 1 1v2m3 0v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6" /></svg>
            </button>
          </div>
        {/each}
      </div>
    {/if}
  </aside>

  <section class="editor">
    {#if !selected}
      <!-- Matches OpenWhispr's main empty state copy -->
      <div class="empty">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" /><path d="M14 2v6h6M9 13h6M9 17h4" /></svg>
        <h2>No notes here yet</h2>
        <p>Create your first note to start writing</p>
        <button class="primary" onclick={newNote}>+ Create note</button>
      </div>
    {:else}
      <div class="edhead">
        <input
          class="title"
          placeholder="Untitled note"
          bind:value={selected.title}
          oninput={queueSave}
        />
        <div class="edactions">
          <!-- ActionPicker split button (OpenWhispr ActionPicker.tsx): left half
               runs the last-used action, the chevron opens the action menu. -->
          <div class="picker">
            <button
              class="enhance runhalf"
              onclick={() => runAction(activeAction)}
              disabled={!activeAction || enhancing.has(selected.id) || !selected.content?.trim()}
              title={activeAction ? `Run "${activeAction.name}"` : 'No actions'}
            >
              {#if enhancing.has(selected.id)}
                <span class="spin"></span> Running…
              {:else}
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3v3M12 18v3M3 12h3M18 12h3M5.6 5.6l2.1 2.1M16.3 16.3l2.1 2.1M18.4 5.6l-2.1 2.1M7.7 16.3l-2.1 2.1" /></svg>
                {activeAction?.name ?? 'Enhance'}
              {/if}
            </button>
            <button
              class="enhance chevron"
              aria-label="Select action"
              disabled={enhancing.has(selected.id)}
              onclick={(e) => {
                e.stopPropagation();
                pickerOpen = !pickerOpen;
              }}
            >
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m6 9 6 6 6-6" /></svg>
            </button>
            {#if pickerOpen}
              <div class="menu" role="menu">
                {#each actions as a (a.id)}
                  <button class="mitem" class:current={a.id === activeAction?.id} role="menuitem" onclick={() => runAction(a)}>
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3v3M12 18v3M3 12h3M18 12h3M5.6 5.6l2.1 2.1M16.3 16.3l2.1 2.1M18.4 5.6l-2.1 2.1M7.7 16.3l-2.1 2.1" /></svg>
                    <span class="mtext">
                      <span class="mname">{a.name}</span>
                      {#if a.description}<span class="mdesc">{a.description}</span>{/if}
                    </span>
                  </button>
                {/each}
                <div class="msep"></div>
                <button
                  class="mitem manage"
                  role="menuitem"
                  onclick={() => {
                    pickerOpen = false;
                    actionsOpen = true;
                  }}
                >
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M20 7h-9M14 17H5" /><circle cx="17" cy="17" r="3" /><circle cx="7" cy="7" r="3" /></svg>
                  Manage actions
                </button>
              </div>
            {/if}
          </div>
        </div>
      </div>

      {#if selected.enhancedContent}
        <div class="tabs">
          <button class="tab" class:active={tab === 'raw'} onclick={() => (tab = 'raw')}>Raw</button>
          <button class="tab" class:active={tab === 'enhanced'} onclick={() => (tab = 'enhanced')}>
            Enhanced
            {#if isStale}<span class="staledot" title="Note changed since enhancement — re-run Enhance"></span>{/if}
          </button>
          <span class="tabspacer"></span>
          <button class="copy" onclick={copyEnhanced}>{copied ? 'Copied!' : 'Copy'}</button>
        </div>
      {/if}

      {#if error}
        <p class="errline">{error}</p>
      {/if}

      {#if tab === 'enhanced' && selected.enhancedContent}
        <!-- eslint-disable-next-line svelte/no-at-html-tags — renderMarkdown
             escapes ALL input before transforming (lib/markdown.js) -->
        <div class="rendered">{@html renderMarkdown(selected.enhancedContent)}</div>
      {:else}
        <textarea
          class="raw"
          placeholder="Write or dictate rough notes here — markdown welcome…"
          bind:value={selected.content}
          oninput={queueSave}
        ></textarea>
      {/if}
    {/if}
  </section>
</div>

<style>
  .notes {
    flex: 1 1 auto;
    display: flex;
    min-height: 0;
  }

  .list {
    flex: 0 0 240px;
    display: flex;
    flex-direction: column;
    min-height: 0;
    border-right: 1px solid var(--yap-border-subtle);
    background: var(--yap-s0, var(--yap-s1));
  }
  .tools {
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 12px 8px 4px;
  }
  .tool {
    display: flex;
    align-items: center;
    gap: 9px;
    width: 100%;
    height: 30px;
    padding: 0 10px;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-muted);
    font: inherit;
    font-size: 12px;
    text-align: left;
    cursor: pointer;
  }
  .tool:hover,
  .tool.on {
    background: var(--yap-s2);
    color: var(--yap-fg);
  }
  .tool svg {
    width: 13px;
    height: 13px;
    flex: 0 0 auto;
  }
  .searchinput {
    margin: 4px 10px 2px;
    padding: 5px 9px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r-sm);
    background: var(--yap-s1);
    color: var(--yap-fg);
    font: inherit;
    font-size: 12px;
  }
  .searchinput:focus {
    outline: none;
    border-color: var(--yap-primary);
  }

  .seccap {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 18px 4px;
    font-size: 10px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.09em;
    color: var(--yap-muted-55);
  }
  .secadd {
    display: inline-flex;
    width: 18px;
    height: 18px;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: var(--yap-r-sm);
    background: none;
    color: var(--yap-muted-55);
    cursor: pointer;
  }
  .secadd:hover {
    background: var(--yap-s2);
    color: var(--yap-fg);
  }
  .secadd svg {
    width: 11px;
    height: 11px;
  }
  .folderlist {
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 0 8px;
  }
  .folder {
    display: flex;
    align-items: center;
    gap: 9px;
    width: 100%;
    height: 30px;
    padding: 0 10px;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-muted);
    font: inherit;
    font-size: 12px;
    text-align: left;
    cursor: pointer;
  }
  .folder:hover {
    background: var(--yap-s2);
    color: var(--yap-fg);
  }
  .folder.active {
    background: var(--yap-primary-wash);
    color: var(--yap-fg);
    font-weight: 600;
  }
  .folder.active svg {
    color: var(--yap-primary);
  }
  .folder svg {
    width: 13px;
    height: 13px;
    flex: 0 0 auto;
  }

  .listempty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    padding: 26px 14px;
    text-align: center;
  }
  .listempty svg {
    width: 26px;
    height: 26px;
    color: var(--yap-muted-55);
    opacity: 0.7;
  }
  .listempty p {
    margin: 0;
    font-size: 11.5px;
    color: var(--yap-muted-55);
  }
  .createbtn {
    margin-top: 4px;
    height: 27px;
    padding: 0 12px;
    border: 1px solid var(--yap-primary);
    border-radius: var(--yap-r);
    background: var(--yap-primary-wash);
    color: var(--yap-primary);
    font: inherit;
    font-size: 11.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .createbtn:hover {
    background: var(--yap-primary);
    color: var(--yap-primary-fg);
  }
  .items {
    flex: 1 1 auto;
    overflow-y: auto;
    min-height: 0;
    padding: 0 8px 12px;
  }
  .item {
    display: flex;
    align-items: flex-start;
    gap: 4px;
    padding: 8px 10px;
    border-radius: var(--yap-r);
    cursor: pointer;
  }
  .item:hover {
    background: var(--yap-s2);
  }
  .item.active {
    background: var(--yap-primary-wash);
  }
  .itembody {
    flex: 1 1 auto;
    min-width: 0;
  }
  .ititle {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0;
    font-size: 12.5px;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .imeta {
    margin: 2px 0 0;
    font-size: 10.5px;
    color: var(--yap-muted-55);
  }
  .enhtag {
    color: var(--yap-primary);
  }
  .idel {
    display: inline-flex;
    width: 22px;
    height: 22px;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: var(--yap-r-sm);
    background: none;
    color: var(--yap-muted-55);
    cursor: pointer;
    opacity: 0;
    flex: 0 0 auto;
  }
  .item:hover .idel {
    opacity: 1;
  }
  .idel:hover {
    color: #ef4444;
  }
  .idel svg {
    width: 12px;
    height: 12px;
  }

  .editor {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    padding: 18px 22px 20px;
  }
  .edhead {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 10px;
  }
  .title {
    flex: 1 1 auto;
    min-width: 0;
    border: none;
    background: none;
    color: var(--yap-fg);
    font: inherit;
    font-size: 17px;
    font-weight: 700;
    letter-spacing: -0.01em;
  }
  .title:focus {
    outline: none;
  }
  .title::placeholder {
    color: var(--yap-muted-55);
  }
  .edactions {
    flex: 0 0 auto;
  }
  .picker {
    position: relative;
    display: inline-flex;
  }
  .enhance {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 30px;
    padding: 0 13px;
    border: none;
    background: var(--yap-primary);
    color: var(--yap-primary-fg);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .enhance.runhalf {
    border-radius: var(--yap-r) 0 0 var(--yap-r);
  }
  .enhance.chevron {
    border-radius: 0 var(--yap-r) var(--yap-r) 0;
    border-left: 1px solid rgba(0, 0, 0, 0.25);
    padding: 0 7px;
  }
  .enhance:hover:not(:disabled) {
    background: var(--yap-primary-hover);
  }
  .enhance:disabled {
    opacity: 0.55;
    cursor: default;
  }
  .enhance svg {
    width: 13px;
    height: 13px;
  }
  .enhance.chevron svg {
    width: 11px;
    height: 11px;
  }
  .menu {
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    z-index: 20;
    min-width: 230px;
    padding: 5px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s1);
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.45);
  }
  .mitem {
    display: flex;
    align-items: flex-start;
    gap: 9px;
    width: 100%;
    padding: 7px 9px;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-fg);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .mitem:hover {
    background: var(--yap-s2);
  }
  .mitem.current {
    background: var(--yap-primary-wash);
  }
  .mitem svg {
    width: 12px;
    height: 12px;
    flex: 0 0 auto;
    margin-top: 2px;
    color: var(--yap-primary);
  }
  .mitem.manage {
    color: var(--yap-muted);
    font-size: 11.5px;
    align-items: center;
  }
  .mitem.manage svg {
    color: var(--yap-muted-55);
    margin-top: 0;
  }
  .mtext {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .mname {
    font-size: 12px;
    font-weight: 600;
  }
  .mdesc {
    font-size: 10.5px;
    color: var(--yap-muted-55);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .msep {
    height: 1px;
    margin: 4px 6px;
    background: var(--yap-border-subtle);
  }
  .spin {
    width: 11px;
    height: 11px;
    border: 2px solid currentColor;
    border-top-color: transparent;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .tabs {
    display: flex;
    align-items: center;
    gap: 4px;
    margin-bottom: 10px;
  }
  .tab {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 26px;
    padding: 0 12px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
    color: var(--yap-muted);
    font: inherit;
    font-size: 11.5px;
    cursor: pointer;
  }
  .tab.active {
    border-color: var(--yap-primary);
    background: var(--yap-primary-wash);
    color: var(--yap-fg);
    font-weight: 600;
  }
  .tabspacer {
    flex: 1;
  }
  .copy {
    border: none;
    background: none;
    color: var(--yap-primary);
    font: inherit;
    font-size: 11.5px;
    cursor: pointer;
  }
  .copy:hover {
    text-decoration: underline;
  }

  .staledot {
    display: inline-block;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: #f5a524;
    flex: 0 0 auto;
  }

  .raw {
    flex: 1 1 auto;
    min-height: 0;
    resize: none;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
    color: var(--yap-fg);
    font: inherit;
    font-size: 13px;
    line-height: 1.65;
    padding: 14px 16px;
  }
  .raw:focus {
    outline: none;
    border-color: var(--yap-primary);
  }
  .raw::placeholder {
    color: var(--yap-muted-55);
  }

  .rendered {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
    padding: 14px 18px;
    font-size: 13px;
    line-height: 1.65;
  }
  .rendered :global(h2),
  .rendered :global(h3),
  .rendered :global(h4),
  .rendered :global(h5) {
    margin: 14px 0 6px;
    line-height: 1.3;
  }
  .rendered :global(h2) {
    font-size: 15px;
  }
  .rendered :global(h3) {
    font-size: 13.5px;
  }
  .rendered :global(p) {
    margin: 6px 0;
  }
  .rendered :global(ul),
  .rendered :global(ol) {
    margin: 6px 0;
    padding-left: 22px;
  }
  .rendered :global(li) {
    margin: 3px 0;
  }
  .rendered :global(li.task) {
    list-style: none;
    margin-left: -18px;
  }
  .rendered :global(code) {
    font-family: ui-monospace, Consolas, monospace;
    font-size: 12px;
    background: var(--yap-s1);
    border-radius: 4px;
    padding: 1px 5px;
  }

  .empty {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    text-align: center;
    color: var(--yap-muted);
    max-width: 380px;
    margin: 0 auto;
  }
  .empty svg {
    width: 34px;
    height: 34px;
    color: var(--yap-muted-55);
    margin-bottom: 4px;
  }
  .empty h2 {
    margin: 0;
    font-size: 15px;
    color: var(--yap-fg);
  }
  .empty p {
    margin: 0;
    font-size: 12.5px;
    line-height: 1.6;
  }
  .primary {
    margin-top: 10px;
    height: 32px;
    padding: 0 16px;
    border: none;
    border-radius: var(--yap-r);
    background: var(--yap-primary);
    color: var(--yap-primary-fg);
    font: inherit;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .primary:hover {
    background: var(--yap-primary-hover);
  }

  .errline {
    margin: 0 0 10px;
    font-size: 12px;
    color: #ef4444;
    line-height: 1.5;
  }
</style>
