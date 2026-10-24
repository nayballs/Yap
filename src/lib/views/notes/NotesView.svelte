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
  // safe rendered Enhanced view (core/markdown.js) — a rich editor
  // (Milkdown/CodeMirror) comes later.
  //
  // This file is the view: it makes its state (notesState.svelte.js: the
  // notes, the open note, the recorder, the actions, the summary and the
  // chat), follows Rust's events while Notes is open, and lays out the list
  // (NoteList) beside the editor (NoteEditor and its parts).
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';
  import { initMeetingAudio, onJobDone, stopLine } from '../../stores/meetingAudio.svelte.js';
  import { stopReading } from '../../stores/speech.svelte.js';
  import { noteRequest } from '../../stores/meetingDetect.svelte.js';
  import { createNotesState } from './notesState.svelte.js';
  import ActionManager from './ActionManager.svelte';
  import NoteList from './NoteList.svelte';
  import NoteEditor from './NoteEditor.svelte';

  // `onopensettings(section)` opens the Settings modal (ControlPanel).
  let { onopensettings = null } = $props();

  const notes = createNotesState();

  onMount(() => {
    notes.refreshList();
    notes.refreshActions();
    const unlisteners = [];
    // A meeting transcribed again (or what a crash left): its new lines.
    initMeetingAudio();
    unlisteners.push(onJobDone((job) => job.noteId === notes.selected?.id && notes.reloadSelected()));
    invoke('meeting_state')
      .then((s) => {
        notes.meeting = s || { recording: false };
        if (notes.meeting.recording) notes.startElapsed(notes.meeting.elapsedSecs || 0);
      })
      .catch(() => {});
    listen('yap-meeting-state', (e) => notes.onMeetingState(e.payload)).then((u) => unlisteners.push(u));
    listen('yap-meeting-segment', (e) => notes.onMeetingSegment(e.payload)).then((u) => unlisteners.push(u));
    listen('yap-meeting-digest', (e) => notes.onMeetingDigest(e.payload)).then((u) => unlisteners.push(u));
    listen('yap-meeting-summary', (e) => notes.onSummary(e.payload)).then((u) => unlisteners.push(u));
    listen('yap-note-changed', (e) => notes.onNoteChanged(e.payload)).then((u) => unlisteners.push(u));
    // (A recording's warnings are notices from notices/meetingGuard.js, in any view.)
    // The local API bridge (Integrations) can create/update/delete notes from
    // outside the app — refresh the list so external edits show up live.
    listen('yap-notes-changed', () => notes.refreshList()).then((u) => unlisteners.push(u));
    // A recording made a note a meeting note and filed it in Meetings
    // (commands.rs `meeting_start` → notes::file_as_meeting).
    listen('yap-note-filed', (e) => notes.onNoteFiled(e.payload)).then((u) => unlisteners.push(u));
    // "Started by mistake?" → Discard (here or in the notepad).
    listen('yap-note-deleted', (e) => {
      if (notes.selected?.id === e.payload?.id) notes.selected = null;
      notes.refreshList();
    }).then((u) => unlisteners.push(u));
    return () => {
      notes.flushSave();
      notes.stopElapsed();
      // Leaving Notes: an answer being read stops, and a line playing.
      stopReading('notes:');
      stopLine();
      unlisteners.forEach((u) => u && u());
    };
  });

  // Call detection (meetingDetect.svelte.js) opens a note here: the meeting
  // note "Record notes" just started, or the one to wrap up — "Stop and
  // summarise" ends it right here (endMeeting), so the action plan is
  // written as for "End meeting & summarise".
  $effect(() => {
    const req = noteRequest.pending;
    if (!req) return;
    noteRequest.pending = null;
    notes.openRequested(req);
  });
</script>

<!-- A click anywhere else closes the folder chip's menu and the action
     picker's (state that outlives the editor's parts). -->
<svelte:window
  onclick={() => {
    notes.pickerOpen = false;
    notes.folderMenuOpen = false;
  }}
/>

<ActionManager bind:open={notes.actionsOpen} onchanged={notes.refreshActions} />

<div class="notes">
  <NoteList {notes} />
  <NoteEditor {notes} {onopensettings} />
</div>

<style>
  .notes {
    flex: 1 1 auto;
    display: flex;
    min-height: 0;
  }
</style>
