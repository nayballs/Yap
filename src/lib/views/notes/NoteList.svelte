<script>
  // The Notes view's list pane (OpenWhispr's sidebar): New note, Search
  // notes, Actions; the folders (renamed in place, their right-click and ⋯
  // menus: Rename, Delete…); the active folder's notes, newest first, each
  // with its menu (Rename, Move to ▸, Read aloud, Copy as Markdown,
  // Export…, Transcribe again…, Delete…). Every delete asks first.
  import { invoke } from '@tauri-apps/api/core';
  import { toast } from '../../ui/toast.svelte.js';
  import { openContextMenu, menuKey, menuOpenFor, askConfirm, devInspect } from '../../ui/contextMenu.svelte.js';
  import { showSaveError } from '../../notices/saveErrors.js';
  import { refreshAudio, cantTranscribeAgain, askTranscribeAgain } from '../../stores/meetingAudio.svelte.js';
  import { displayTitle, relTime, dateOf } from './noteFormat.js';

  /** `notes`: the view's state (notesState.svelte.js). */
  let { notes } = $props();

  // Search notes (the OpenWhispr sidebar action): filters the list pane.
  let searchOpen = $state(false);
  let query = $state('');
  let searchEl = $state(null);
  // Inline "new folder" input (the + next to FOLDERS).
  let addingFolder = $state(false);

  // Notes shown in the list pane: the active folder, filtered by search.
  const shownNotes = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return notes.notes.filter(
      (n) =>
        (n.folder || 'Personal') === notes.activeFolder &&
        (!q || (n.title || '').toLowerCase().includes(q) || (n.preview || '').toLowerCase().includes(q))
    );
  });
  const folderCounts = $derived.by(() => {
    const counts = {};
    for (const n of notes.notes) counts[n.folder || 'Personal'] = (counts[n.folder || 'Personal'] || 0) + 1;
    return counts;
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
    const name = notes.newFolderName.trim();
    addingFolder = false;
    notes.newFolderName = '';
    if (!name) return;
    try {
      notes.folders = await invoke('notes_folder_create', { name });
      notes.activeFolder = name;
    } catch (e) {
      // Made, but not saved yet (Yap keeps it and says so).
      if (showSaveError(e)) {
        notes.activeFolder = name;
        notes.refreshList();
      }
    }
  }

  // ---- Right-click menus (OpenWhispr's folder and note dropdowns, as
  // context menus too: ui/ContextMenu.svelte) ----

  // Yap files notes into these two: they can't be renamed or deleted (Rust
  // refuses too, notes.rs `is_protected_folder`).
  const PROTECTED = { personal: 'new notes go here.', meetings: 'meeting notes go here.' };
  const protectedReason = (f) => PROTECTED[f.trim().toLowerCase()] ?? null;

  /**
   * Open a row's menu: at the pointer (a right-click), under its ⋯ button,
   * or at the row (the ContextMenu key, Shift+F10) — the app's one menu
   * (ui/contextMenu.svelte.js), which cancels the event, so WebView2 never
   * shows its own.
   */
  function openMenu(e, items, label, row) {
    openContextMenu(e, items, { label, row });
  }

  // Folders: Rename (inline), Delete… (confirm; its notes go to Personal).
  let renamingFolder = $state(null);
  let renameValue = $state('');
  let folderEls = $state({});
  function folderMenu(e, f) {
    const reason = protectedReason(f);
    const items = [
      { label: 'Rename', icon: 'pencil', disabled: !!reason, onselect: () => startFolderRename(f) },
      { separator: true },
      { label: 'Delete…', icon: 'trash', danger: true, disabled: !!reason, onselect: () => askDeleteFolder(f) },
    ];
    if (reason) items.push({ note: `Yap's own folder: ${reason}` });
    // A right-click picks the folder too, as a click does.
    if (e.type === 'contextmenu' && !devInspect(e)) notes.activeFolder = f;
    openMenu(e, items, `Folder ${f}`, folderEls[f]);
  }
  function startFolderRename(f) {
    renamingFolder = f;
    renameValue = f;
  }
  async function finishFolderRename(save = true) {
    const from = renamingFolder;
    if (from == null) return;
    renamingFolder = null;
    const to = renameValue.trim();
    renameValue = '';
    if (!save || !to || to === from) {
      setTimeout(() => folderEls[from]?.focus({ preventScroll: true }), 0);
      return;
    }
    try {
      notes.folders = await invoke('notes_folder_rename', { from, to });
    } catch (e) {
      if (!showSaveError(e)) {
        toast({ title: "Couldn't rename the folder", description: String(e), variant: 'destructive' });
        return;
      }
    }
    if (notes.activeFolder === from) notes.activeFolder = to;
    if (notes.selected?.folder === from) notes.selected.folder = to;
    await notes.refreshList();
    setTimeout(() => folderEls[to]?.focus({ preventScroll: true }), 0);
  }
  function focusOnMount(el) {
    el.focus();
    el.select();
  }

  // Every delete here asks first (the window's confirm dialog, askConfirm).
  function askDeleteFolder(f) {
    const count = folderCounts[f] || 0;
    askConfirm({
      title: `Delete “${f}”?`,
      body: count === 0 ? 'The folder is empty.' : `${count === 1 ? 'Its note' : `Its ${count} notes`} will move to Personal.`,
      run: () => deleteFolder(f),
      returnFocus: folderEls[f],
    });
  }
  async function deleteFolder(f) {
    try {
      notes.folders = await invoke('notes_folder_delete', { name: f });
    } catch (e) {
      if (!showSaveError(e)) {
        toast({ title: "Couldn't delete the folder", description: String(e), variant: 'destructive' });
        return false;
      }
    }
    if (notes.activeFolder === f) notes.activeFolder = 'Personal';
    if (notes.selected?.folder === f) notes.selected.folder = 'Personal';
    await notes.refreshList();
    setTimeout(() => folderEls.Personal?.focus({ preventScroll: true }), 0);
    return true;
  }
  // Notes: Rename (the title), Move to ▸, Copy as Markdown, Export…, Delete….
  let itemEls = $state({});
  function noteMenu(e, n) {
    const here = n.folder || 'Personal';
    const recording = notes.meeting.recording && notes.meeting.noteId === n.id;
    // A right-click opens the note too, as a click does — unless that would
    // throw away a question being asked about the open one.
    if (
      e.type === 'contextmenu' &&
      !devInspect(e) &&
      notes.selected?.id !== n.id &&
      !notes.chatInput.trim() &&
      !notes.chatThread.length &&
      !notes.asking
    ) {
      notes.select(n.id);
    }
    openMenu(
      e,
      [
        { label: 'Rename', icon: 'pencil', onselect: () => notes.renameNote(n.id) },
        {
          label: 'Move to',
          icon: 'folder',
          items: notes.folders.map((f) => ({
            label: f,
            checked: f === here,
            disabled: f === here,
            onselect: () => notes.moveToFolder(f, n.id),
          })),
        },
        { separator: true },
        notes.readingNote(n.id)
          ? { label: 'Stop reading', icon: 'stop', onselect: () => notes.readNote(n.id) }
          : { label: 'Read aloud', icon: 'play', onselect: () => notes.readNote(n.id) },
        { label: 'Copy as Markdown', icon: 'copy', onselect: () => notes.copyNoteMarkdown(n.id) },
        { label: 'Export…', icon: 'download', onselect: () => notes.exportNote(n.id) },
        // A meeting kept on disk (meetings/meeting_audio.rs): on the speech model in use now.
        ...(n.noteType === 'meeting'
          ? [
              {
                label: 'Transcribe again…',
                icon: 'sync',
                disabled: !!cantTranscribeAgain(n, recording),
                onselect: () => transcribeAgain(n),
              },
              ...(cantTranscribeAgain(n, recording) ? [{ note: cantTranscribeAgain(n, recording) }] : []),
            ]
          : []),
        { separator: true },
        {
          label: 'Delete…',
          icon: 'trash',
          danger: true,
          onselect: () =>
            askConfirm({
              title: `Delete “${displayTitle(n)}”?`,
              body: recording
                ? 'The recording stops first. Its transcript, your notes and the summary go for good.'
                : n.noteType === 'meeting'
                  ? 'Its transcript, your notes and the summary go for good.'
                  : 'The note goes for good.',
              run: () => notes.removeNote(n.id),
              returnFocus: itemEls[n.id],
            }),
        },
      ],
      `Note ${displayTitle(n)}`,
      itemEls[n.id]
    );
  }

  // "Transcribe again…" (the note's menu): asked first (meetingAudio.svelte.js).
  async function transcribeAgain(n) {
    if (notes.selected?.id === n.id) notes.flushSave();
    await refreshAudio(n.id);
    askTranscribeAgain(n, { title: displayTitle(n), returnFocus: itemEls[n.id] });
  }
</script>

<aside class="list">
  <!-- Action rows, OpenWhispr order: New note / Search notes -->
  <div class="tools">
    <button class="tool" onclick={notes.newNote}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M13.4 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-7.4" /><path d="M18.4 2.6a2 2 0 0 1 2.8 2.8L13 13.6 9 14.6l1-4z" /></svg>
      New note
    </button>
    <button class="tool" class:on={searchOpen} onclick={toggleSearch}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="11" cy="11" r="7" /><path d="m21 21-4.3-4.3" /></svg>
      Search notes
    </button>
    <button class="tool" onclick={() => (notes.actionsOpen = true)}>
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
    {#each notes.folders as f (f)}
      {#if renamingFolder === f}
        <!-- Rename in place (OpenWhispr's): Enter or leaving it saves, Esc cancels. -->
        <div class="renamerow">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 20h16a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.7-.9L9.2 3.9A2 2 0 0 0 7.5 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2z" /></svg>
          <input
            class="renameinput"
            aria-label={`Rename folder ${f}`}
            bind:value={renameValue}
            use:focusOnMount
            onkeydown={(e) => {
              if (e.key === 'Enter') finishFolderRename(true);
              if (e.key === 'Escape') {
                e.stopPropagation();
                finishFolderRename(false);
              }
            }}
            onblur={() => finishFolderRename(true)}
          />
        </div>
      {:else}
        <div class="folderrow" class:menuopen={menuOpenFor(folderEls[f])}>
          <button
            bind:this={folderEls[f]}
            class="folder"
            class:active={notes.activeFolder === f}
            aria-haspopup="menu"
            onclick={() => (notes.activeFolder = f)}
            oncontextmenu={(e) => folderMenu(e, f)}
            onkeydown={(e) => menuKey(e, (ev) => folderMenu(ev, f))}
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 20h16a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.7-.9L9.2 3.9A2 2 0 0 0 7.5 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2z" /></svg>
            <span class="fname">{f}</span>
            {#if folderCounts[f]}<span class="fcount">{folderCounts[f]}</span>{/if}
          </button>
          <button
            class="more"
            title="More"
            aria-label={`More for folder ${f}`}
            aria-haspopup="menu"
            data-more
            onclick={(e) => folderMenu(e, f)}
          >
            <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><circle cx="5" cy="12" r="1.7" /><circle cx="12" cy="12" r="1.7" /><circle cx="19" cy="12" r="1.7" /></svg>
          </button>
        </div>
      {/if}
    {/each}
    {#if addingFolder}
      <input
        class="searchinput"
        placeholder="Folder name…"
        bind:value={notes.newFolderName}
        onkeydown={(e) => {
          if (e.key === 'Enter') createFolder();
          if (e.key === 'Escape') {
            addingFolder = false;
            notes.newFolderName = '';
          }
        }}
        onblur={createFolder}
      />
    {/if}
  </div>

  <!-- NOTES -->
  <div class="seccap">
    <span>Notes</span>
    <button class="secadd" title="New note" aria-label="New note" onclick={notes.newNote}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
    </button>
  </div>
  {#if shownNotes.length === 0}
    <div class="listempty">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" /><path d="M14 2v6h6M9 13h6M9 17h4" /></svg>
      <p>{query.trim() ? 'No matching notes' : 'No notes in this folder'}</p>
      {#if !query.trim()}
        <button class="createbtn" onclick={notes.newNote}>+ Create note</button>
      {/if}
    </div>
  {:else}
    <div class="items">
      {#each shownNotes as n (n.id)}
        <div
          bind:this={itemEls[n.id]}
          class="item"
          class:active={notes.selected?.id === n.id}
          class:menuopen={menuOpenFor(itemEls[n.id])}
          role="button"
          tabindex="0"
          aria-haspopup="menu"
          onclick={() => notes.select(n.id)}
          oncontextmenu={(e) => noteMenu(e, n)}
          onkeydown={(e) => {
            if (e.target !== e.currentTarget) return;
            if (e.key === 'Enter' || e.key === ' ') notes.select(n.id);
            else menuKey(e, (ev) => noteMenu(ev, n));
          }}
        >
          <div class="itembody">
            <p class="ititle">
              <span class="ittext">{displayTitle(n)}</span>
              {#if n.stale}<span class="staledot" title="Your notes changed since this summary"></span>{/if}
              <span class="reltime">{relTime(n.updatedTs)}</span>
            </p>
            <p class="imeta">
              {#if n.noteType === 'meeting'}<span class="enhtag">meeting</span> · {/if}
              {#if n.hasEnhanced}<span class="enhtag">summary</span>{:else}{dateOf(n.updatedTs)}{/if}
            </p>
          </div>
          <button
            class="more"
            title="More"
            aria-label={`More for ${displayTitle(n)}`}
            aria-haspopup="menu"
            data-more
            onclick={(e) => noteMenu(e, n)}
          >
            <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><circle cx="5" cy="12" r="1.7" /><circle cx="12" cy="12" r="1.7" /><circle cx="19" cy="12" r="1.7" /></svg>
          </button>
        </div>
      {/each}
    </div>
  {/if}
</aside>

<style>
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
    width: 20px;
    height: 20px;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: var(--yap-r-sm);
    background: none;
    color: var(--yap-muted);
    cursor: pointer;
  }
  .secadd:hover {
    background: var(--yap-s2);
    color: var(--yap-fg);
  }
  .secadd svg {
    width: 13px;
    height: 13px;
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
    background: var(--yap-primary-tint);
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
  }
  .ittext {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .reltime {
    flex: 0 0 auto;
    font-size: 10px;
    font-weight: 500;
    color: var(--yap-muted-55);
  }
  .fname {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .fcount {
    flex: 0 0 auto;
    font-size: 10px;
    color: var(--yap-muted-55);
  }
  .imeta {
    margin: 2px 0 0;
    font-size: 10.5px;
    color: var(--yap-muted-55);
  }
  .enhtag {
    color: var(--yap-primary);
  }
  /* The ⋯ on a row (its menu without right-clicking): shown on hover, on
     focus and while its menu is open. On a folder it takes the count's place. */
  .more {
    display: inline-flex;
    width: 22px;
    height: 22px;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: var(--yap-r-sm);
    background: none;
    color: var(--yap-muted);
    cursor: pointer;
    opacity: 0;
    flex: 0 0 auto;
  }
  .more:hover {
    background: var(--yap-s3);
    color: var(--yap-fg);
  }
  .more:focus-visible {
    opacity: 1;
    outline: 1.5px solid var(--yap-primary-line);
    outline-offset: -1px;
  }
  .more svg {
    width: 13px;
    height: 13px;
  }
  .item:hover .more,
  .item:focus-within .more,
  .item.menuopen .more {
    opacity: 1;
  }
  .item.menuopen {
    background: var(--yap-s2);
  }
  .item.active.menuopen {
    background: var(--yap-primary-wash);
  }
  .folderrow {
    position: relative;
    display: flex;
  }
  .folderrow .more {
    position: absolute;
    top: 50%;
    right: 5px;
    transform: translateY(-50%);
  }
  .folderrow:hover .more,
  .folderrow:focus-within .more,
  .folderrow.menuopen .more {
    opacity: 1;
  }
  .folderrow:hover .fcount,
  .folderrow:focus-within .fcount,
  .folderrow.menuopen .fcount {
    visibility: hidden;
  }
  .folderrow.menuopen .folder:not(.active) {
    background: var(--yap-s2);
    color: var(--yap-fg);
  }
  .folderrow:hover .folder,
  .folderrow:focus-within .folder,
  .folderrow.menuopen .folder {
    padding-right: 30px;
  }
  /* The folder being renamed: the row's own layout, its name a text box. */
  .renamerow {
    position: relative;
    display: flex;
    align-items: center;
  }
  .renamerow svg {
    position: absolute;
    left: 10px;
    width: 13px;
    height: 13px;
    color: var(--yap-primary);
    pointer-events: none;
  }
  .renameinput {
    width: 100%;
    height: 30px;
    box-sizing: border-box;
    padding: 0 9px 0 31px;
    border: 1px solid var(--yap-primary);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
    color: var(--yap-fg);
    font: inherit;
    font-size: 12px;
    box-shadow: 0 0 0 3px var(--yap-primary-wash);
  }
  .renameinput:focus {
    outline: none;
  }

  /* Your notes changed since this summary. */
  .staledot {
    display: inline-block;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--yap-key-bg);
    flex: 0 0 auto;
  }
</style>
