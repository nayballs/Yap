<script>
  // The open note's chip row (OpenWhispr NoteEditor header): its date, its
  // attendees (+ Add attendees, a popover), its folder (a menu: move it, or
  // a new folder), then at the right: the meeting notepad, Record / Pause
  // (the meeting recorder: mic "You" + system audio "Them"; "End meeting &
  // summarise" is in the bottom bar), Read aloud and Export.
  import { invoke } from '@tauri-apps/api/core';
  import { toast } from '../../ui/toast.svelte.js';
  import { showSaveError } from '../../notices/saveErrors.js';
  import { chipDate, fmtElapsed } from './noteFormat.js';

  /** `notes`: the view's state (notesState.svelte.js). */
  let { notes } = $props();

  let addingAttendee = $state(false);
  let attendeeInput = $state('');
  let attendeeEl = $state(null);
  let addingFolderInMenu = $state(false);

  async function createFolderAndMove() {
    const name = notes.newFolderName.trim();
    addingFolderInMenu = false;
    notes.newFolderName = '';
    if (!name) return;
    try {
      notes.folders = await invoke('notes_folder_create', { name });
    } catch (e) {
      if (!showSaveError(e)) {
        toast({ title: "Couldn't create folder", description: String(e), variant: 'destructive' });
        return;
      }
    }
    await notes.moveToFolder(name);
  }

  function persistParticipants() {
    if (!notes.selected) return;
    invoke('note_update', { id: notes.selected.id, participants: notes.selected.participants || [] }).catch((e) =>
      showSaveError(e)
    );
  }
  function addAttendee() {
    const name = attendeeInput.trim();
    attendeeInput = '';
    if (!name || !notes.selected) return;
    // Popover stays open so several attendees can be added in a row.
    notes.selected.participants = [...(notes.selected.participants || []), name];
    persistParticipants();
  }
  function removeAttendee(i) {
    if (!notes.selected) return;
    notes.selected.participants = (notes.selected.participants || []).filter((_, j) => j !== i);
    persistParticipants();
  }
</script>

<!-- A click anywhere else closes the chips' popovers (the folder menu:
     NotesView.svelte, which outlives this row). -->
<svelte:window
  onclick={() => {
    addingAttendee = false;
    addingFolderInMenu = false;
  }}
/>

<div class="metarow">
  <span class="chip static">
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3" y="4" width="18" height="18" rx="2" /><path d="M16 2v4M8 2v4M3 10h18" /></svg>
    {chipDate(notes.selected.createdTs)}
  </span>
  {#each notes.selected.participants || [] as p, i (p + i)}
    <span class="chip person">
      {p}
      <button class="chipx" aria-label={`Remove ${p}`} onclick={() => removeAttendee(i)}>×</button>
    </span>
  {/each}
  <span class="chipmenuwrap">
    <button
      class="chip"
      onclick={(e) => {
        e.stopPropagation();
        addingAttendee = !addingAttendee;
        notes.folderMenuOpen = false;
        if (addingAttendee) setTimeout(() => attendeeEl?.focus(), 0);
      }}
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="9" cy="8" r="3.2" /><path d="M2.5 20a6.5 6.5 0 0 1 13 0" /><path d="M19 8v6M16 11h6" /></svg>
      Add attendees
    </button>
    {#if addingAttendee}
      <!-- Popover, not an inline morph (OpenWhispr's attendees card) -->
      <div class="chipmenu pop" role="presentation" onclick={(e) => e.stopPropagation()}>
        <input
          class="popinput"
          bind:this={attendeeEl}
          placeholder="Add attendees…"
          bind:value={attendeeInput}
          onkeydown={(e) => {
            if (e.key === 'Enter') addAttendee();
            if (e.key === 'Escape') {
              addingAttendee = false;
              attendeeInput = '';
            }
          }}
        />
        <p class="pophint">Type a name and press Enter to add…</p>
      </div>
    {/if}
  </span>
  <span class="chipmenuwrap">
    <button
      class="chip"
      onclick={(e) => {
        e.stopPropagation();
        notes.folderMenuOpen = !notes.folderMenuOpen;
        addingAttendee = false;
        addingFolderInMenu = false;
      }}
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 20h16a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.7-.9L9.2 3.9A2 2 0 0 0 7.5 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2z" /></svg>
      {notes.selected.folder || 'Personal'}
    </button>
    {#if notes.folderMenuOpen}
      <div class="chipmenu" role="presentation" onclick={(e) => e.stopPropagation()}>
        {#each notes.folders as f (f)}
          <button class="chipmenuitem" class:current={f === notes.selected.folder} role="menuitem" onclick={() => notes.moveToFolder(f)}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 20h16a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.7-.9L9.2 3.9A2 2 0 0 0 7.5 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2z" /></svg>
            <span class="cmname">{f}</span>
            {#if f === notes.selected.folder}
              <svg class="cmcheck" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 12l5 5L20 6" /></svg>
            {/if}
          </button>
        {/each}
        <div class="msep"></div>
        {#if addingFolderInMenu}
          <input
            class="popinput"
            placeholder="Folder name…"
            bind:value={notes.newFolderName}
            onkeydown={(e) => {
              if (e.key === 'Enter') createFolderAndMove();
              if (e.key === 'Escape') {
                addingFolderInMenu = false;
                notes.newFolderName = '';
              }
            }}
          />
        {:else}
          <button class="chipmenuitem newf" onclick={() => (addingFolderInMenu = true)}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
            <span class="cmname">New folder</span>
          </button>
        {/if}
      </div>
    {/if}
  </span>
  <span class="metaspacer"></span>
  <!-- The meeting notepad (docked beside the call) on this note. -->
  {#if notes.selected.noteType === 'meeting' || notes.recordingThisNote}
    <button class="chip" onclick={notes.openNotepad} title="Open the meeting notepad beside your call">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3" y="3" width="18" height="18" rx="2" /><path d="M15 3v18" /><path d="M18 8h0M18 12h0" /></svg>
      Notepad
    </button>
  {/if}
  <!-- Meeting recorder: mic ("You") + system audio ("Them"). Pause
       keeps the note as it is; "End meeting & summarise" (bottom bar)
       ends it, and Rust writes the action plan. -->
  {#if notes.recordingThisNote}
    <button class="chip reclive" onclick={notes.pauseMeeting} aria-label="Pause recording" title="Pause — Resume carries on in this note">
      <span class="recdot"></span>
      {fmtElapsed(notes.elapsed)} · Pause
    </button>
  {:else}
    <button
      class="chip"
      onclick={notes.startMeeting}
      disabled={notes.meeting.recording}
      title={notes.meeting.recording
        ? 'Already recording another note'
        : 'Record this meeting — your mic is "You", the call audio is "Them"'}
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="2" width="6" height="12" rx="3" /><path d="M5 10v1a7 7 0 0 0 14 0v-1" /><path d="M12 18v4" /></svg>
      {notes.selected.transcript?.length ? 'Resume' : 'Record'}
    </button>
  {/if}
  <!-- Read aloud (voice/read_aloud.rs): the reading card on the Yap bar. -->
  <button
    class="iconbtn"
    class:on={notes.readingNote(notes.selected.id)}
    title={notes.readingNote(notes.selected.id) ? 'Stop reading' : 'Read aloud'}
    aria-label={notes.readingNote(notes.selected.id) ? 'Stop reading' : 'Read aloud'}
    aria-pressed={notes.readingNote(notes.selected.id)}
    onclick={() => notes.readNote()}
  >
    {#if notes.readingNote(notes.selected.id)}
      <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><rect x="7" y="7" width="10" height="10" rx="2" /></svg>
    {:else}
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 9.5v5h3.5L12 18.5v-13L7.5 9.5z" /><path d="M15.5 9a4 4 0 0 1 0 6" /><path d="M18.5 6.5a7.5 7.5 0 0 1 0 11" /></svg>
    {/if}
  </button>
  <button class="iconbtn" title="Export as Markdown" aria-label="Export as Markdown" onclick={() => notes.exportNote()}>
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" /><path d="M7 10l5 5 5-5" /><path d="M12 15V3" /></svg>
  </button>
</div>

<style>
  .chip.reclive {
    border-color: var(--yap-danger);
    background: color-mix(in srgb, var(--yap-danger) 8%, transparent);
    color: var(--yap-danger);
    font-family: ui-monospace, Consolas, monospace;
  }
  button.chip:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .recdot {
    display: inline-block;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--yap-danger);
    animation: recpulse 1.2s ease-in-out infinite;
  }
  @keyframes recpulse {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.35;
    }
  }
  .msep {
    height: 1px;
    margin: 4px 6px;
    background: var(--yap-border-subtle);
  }

  /* meta chip row (date · attendees · folder · export) */
  .metarow {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px;
    margin: 2px 0 12px;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 24px;
    padding: 0 9px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
    color: var(--yap-muted);
    font: inherit;
    font-size: 11px;
    cursor: pointer;
    transition:
      border-color var(--yap-dur) ease,
      color var(--yap-dur) ease;
  }
  button.chip:hover {
    border-color: var(--yap-border-hover);
    color: var(--yap-fg);
  }
  .chip.static {
    cursor: default;
  }
  .chip svg {
    width: 11px;
    height: 11px;
    flex: 0 0 auto;
  }
  .chip.person {
    color: var(--yap-fg);
    background: var(--yap-primary-wash);
    border-color: transparent;
    cursor: default;
  }
  .chipx {
    border: none;
    background: none;
    color: var(--yap-muted-55);
    font: inherit;
    font-size: 13px;
    line-height: 1;
    cursor: pointer;
    padding: 0;
  }
  .chipx:hover {
    color: var(--yap-danger);
  }
  .chipmenuwrap {
    position: relative;
    display: inline-flex;
  }
  .chipmenu {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    z-index: 20;
    min-width: 150px;
    padding: 4px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s1);
    box-shadow: var(--yap-shadow-menu);
  }
  /* attendees popover card (OpenWhispr style: input + hint) */
  .chipmenu.pop {
    min-width: 240px;
    padding: 9px;
  }
  .popinput {
    width: 100%;
    height: 28px;
    padding: 0 9px;
    border: 1px solid var(--yap-primary);
    border-radius: var(--yap-r-sm);
    background: var(--yap-s2);
    color: var(--yap-fg);
    font: inherit;
    font-size: 11.5px;
    box-sizing: border-box;
  }
  .popinput:focus {
    outline: none;
  }
  .pophint {
    margin: 8px 0 2px;
    text-align: center;
    font-size: 10.5px;
    color: var(--yap-muted-55);
  }
  .chipmenuitem {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 6px 9px;
    border: none;
    border-radius: var(--yap-r-sm);
    background: none;
    color: var(--yap-fg);
    font: inherit;
    font-size: 11.5px;
    text-align: left;
    cursor: pointer;
  }
  .chipmenuitem:hover {
    background: var(--yap-s2);
  }
  .chipmenuitem.current {
    background: var(--yap-primary-wash);
    font-weight: 600;
  }
  .chipmenuitem svg {
    width: 11px;
    height: 11px;
    flex: 0 0 auto;
    color: var(--yap-muted-55);
  }
  .chipmenuitem.current svg {
    color: var(--yap-primary);
  }
  .cmname {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .cmcheck {
    color: var(--yap-primary) !important;
  }
  .chipmenuitem.newf {
    color: var(--yap-muted);
  }
  .metaspacer {
    flex: 1;
  }
  .iconbtn {
    display: inline-flex;
    width: 26px;
    height: 26px;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: var(--yap-r-sm);
    background: none;
    color: var(--yap-muted-55);
    cursor: pointer;
  }
  .iconbtn:hover {
    background: var(--yap-s2);
    color: var(--yap-fg);
  }
  /* Reading this note aloud: ■ until it stops. */
  .iconbtn.on {
    color: var(--yap-fg);
    background: var(--yap-s2);
  }
  .iconbtn svg {
    width: 14px;
    height: 14px;
  }
</style>
