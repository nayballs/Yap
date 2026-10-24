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
  // (Milkdown/CodeMirror) comes later. Meeting notes: Record / Pause /
  // Resume in the chip row, "End meeting & summarise" in the bottom bar; the
  // built-in Action Plan is then written by Rust (meeting_end.rs), whichever
  // window is open, and shown here as it goes (docs/meetings.md). The meeting
  // notepad (Notepad.svelte) shows the same note: edits sync both ways.
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';
  import { renderMarkdown, markdownToText } from './markdown.js';
  import { toast } from './ui/toast.svelte.js';
  import ActionManager from './ActionManager.svelte';
  import { noteRequest } from './meetingDetect.svelte.js';
  import { summaries, loadSummary, summarise } from './meetingSummary.svelte.js';
  import {
    MEETING_HELPERS,
    meetingHelpers,
    noteMarkdown,
    transcriptItems,
    withLineSpeakers,
    withLineSplice,
    hasLine,
  } from './notepadText.js';
  import SpeakerMenu from './SpeakerMenu.svelte';
  import SpeakerCard from './SpeakerCard.svelte';
  // The refine after a meeting, Try again on a stretch that failed (meeting_refine.rs).
  import MeetingJob from './MeetingJob.svelte';
  import FailedChunk from './FailedChunk.svelte';
  import { jobState } from './meetingAudio.svelte.js';
  import { nextModel, modelLine, whereItRuns } from './aiModel.js';
  import { speech, initSpeech, readAloud, stopReading } from './speech.svelte.js';
  import { openContextMenu, menuKey, menuOpenFor, askConfirm, devInspect } from './ui/contextMenu.svelte.js';
  import { copyText, messageItems } from './itemMenus.js';
  import {
    meetingAudio,
    initMeetingAudio,
    onJobDone,
    refreshAudio,
    lastPart,
    cantTranscribeAgain,
    askTranscribeAgain,
    recoverMeeting,
    canPlay,
    playLine,
    stopLine,
  } from './meetingAudio.svelte.js';

  /** This window's label: the `origin` of its saves and stops. */
  const LABEL = 'settings';

  // `onopensettings(section)` opens the Settings modal (ControlPanel).
  let { onopensettings = null } = $props();

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
  // The built-in "End meeting & summarise" runs (found by kind: users can
  // rename it). Older stores fall back to Meeting Notes.
  const actionPlanAction = $derived(
    actions.find((a) => a.kind === 'actionPlan') ??
      actions.find((a) => a.name === 'Meeting Notes') ??
      null
  );

  async function refreshActions() {
    try {
      actions = (await invoke('notes_actions')) || [];
    } catch {
      actions = [];
    }
  }

  // ---- Meeting recorder (OpenWhispr meetingRecordingStore port) ----
  // meeting = { recording, noteId, elapsedSecs } from the backend; segments
  // stream in live via yap-meeting-segment and land on note.transcript, and
  // the rolling AI digests (meeting_summary.rs) via yap-meeting-digest.
  let meeting = $state({ recording: false });
  let elapsed = $state(0);
  let elapsedTimer = null;
  const recordingThisNote = $derived(meeting.recording && meeting.noteId === selected?.id);
  // The open note's action-plan job (Rust writes it when a meeting ends, or
  // on "Action plan"), and `{ done, total }` while a long meeting's last
  // digests are written first.
  const job = $derived(selected ? summaries.byNote[selected.id] : null);
  const jobRunning = $derived(job?.state === 'running');
  const summaryProgress = $derived(selected ? summaries.progress[selected.id] : null);
  // `{ noteId, message }`: an action needs an AI model set up first.
  let aiMissing = $state(null);
  const needsAi = $derived(
    aiMissing?.noteId === selected?.id
      ? aiMissing.message
      : job?.state === 'needsAi'
        ? job.error
        : null
  );
  // The transcript box shows the live transcript or the AI notes so far.
  let transcriptView = $state('live');
  // While recording, the live transcript follows the newest lines — unless
  // the user scrolled up to read.
  let logEl = $state(null);
  let followLog = true;
  $effect(() => {
    void shownTranscript.length;
    if (logEl && followLog && recordingThisNote) logEl.scrollTop = logEl.scrollHeight;
  });
  // "You" chunks that were the call coming through the speakers (meeting.rs
  // echo check): kept, hidden unless asked for. A dictation made mid-meeting
  // is a `dictated` marker (its words were left out of "You"): shown as a
  // quiet "You dictated here" line.
  let showEcho = $state(false);
  const echoCount = $derived((selected?.transcript || []).filter((s) => s.echo).length);
  const shownTranscript = $derived(
    (selected?.transcript || []).filter((s) => showEcho || !s.echo)
  );
  // The transcript as runs of one speaker (named from the note's speaker
  // map, notepadText.js): a bubble per line, the name on the first.
  const transcriptRuns = $derived(
    transcriptItems(selected?.transcript, [], { showEcho, speakers: selected?.speakers })
  );
  // The menu on a speaker's name: `{ key, voice, lines, start }`.
  let speakerMenu = $state(null);
  function toggleSpeakerMenu(key, voice, lines, start = 'menu') {
    speakerMenu = speakerMenu?.key === key ? null : { key, voice, lines, start };
  }
  // A transcript line's right-click menu (ui/contextMenu.svelte.js), as the
  // meeting notepad's: Copy line, Who said this? (the speaker menu at the
  // line) — not for a line from the speakers.
  let lineEls = $state({});
  function lineMenu(e, run, line) {
    openContextMenu(
      e,
      [
        {
          label: 'Copy line',
          icon: 'copy',
          onselect: () => copyText(line.text),
        },
        canPlay(selected?.id, selected?.transcript?.[line.i])
          ? {
              label: meetingAudio.playing === `${selected.id}:${line.i}` ? 'Stop playing' : 'Play this line',
              icon: meetingAudio.playing === `${selected.id}:${line.i}` ? 'stop' : 'play',
              onselect: () => playLine(selected.id, line.i, selected.transcript[line.i]),
            }
          : null,
        !run.echo
          ? {
              label: 'Who said this?',
              icon: 'person',
              onselect: () => (speakerMenu = { key: `l${line.i}`, voice: run.speaker, lines: [line.i], start: 'lines' }),
            }
          : null,
      ],
      { label: 'Transcript line', row: lineEls[line.i] }
    );
  }

  /** A speaker edit made here: the map, each line's voice (and echo: "It
   *  was me"), and the summary (a name put on "Speaker 2"). */
  function onSpeakerChanged(updated) {
    if (!selected || updated?.id !== selected.id) return;
    selected.speakers = updated.speakers || {};
    const t = updated.transcript || [];
    selected.transcript = (selected.transcript || []).map((s, i) =>
      t[i] ? { ...s, speaker: t[i].speaker || '', speakerLocked: !!t[i].speakerLocked, echo: !!t[i].echo } : s
    );
    if (typeof updated.enhancedContent === 'string') selected.enhancedContent = updated.enhancedContent;
  }
  // "AI notes up to 40:12": where the digests reach, from the meeting's start.
  const digestedUpTo = $derived.by(() => {
    const d = selected?.digests;
    const t0 = selected?.transcript?.[0]?.ts;
    if (!d?.length || t0 == null) return null;
    return clock(d[d.length - 1].endTs - t0);
  });
  function clock(secs) {
    const s = Math.max(0, Math.round(secs));
    const h = Math.floor(s / 3600);
    const m = Math.floor((s % 3600) / 60);
    const sec = String(s % 60).padStart(2, '0');
    return h ? `${h}:${String(m).padStart(2, '0')}:${sec}` : `${m}:${sec}`;
  }
  function digestRange(d) {
    const t0 = selected?.transcript?.[0]?.ts ?? d.startTs;
    return `${clock(d.startTs - t0)}–${clock(d.endTs - t0)}`;
  }

  function startElapsed(from = 0) {
    elapsed = from;
    clearInterval(elapsedTimer);
    elapsedTimer = setInterval(() => (elapsed += 1), 1000);
  }
  function stopElapsed() {
    clearInterval(elapsedTimer);
    elapsedTimer = null;
  }
  function fmtElapsed(s) {
    const m = Math.floor(s / 60);
    const sec = String(s % 60).padStart(2, '0');
    return m >= 60
      ? `${Math.floor(m / 60)}:${String(m % 60).padStart(2, '0')}:${sec}`
      : `${m}:${sec}`;
  }

  async function startMeeting() {
    if (!selected || meeting.recording) return;
    flushSave();
    error = null;
    if (aiMissing?.noteId === selected.id) aiMissing = null;
    const resuming = !!selected.transcript?.length;
    try {
      await invoke('meeting_start', { noteId: selected.id });
      // state event updates `meeting`; start the local timer optimistically
      // The ● timer chip (and the notepad, when it opens) says it.
      startElapsed(0);
    } catch (e) {
      error = String(e);
      toast({ title: "Couldn't start recording", description: String(e), variant: 'destructive' });
    }
  }
  // Pause: stop recording without writing the action plan (Resume carries
  // on in the same note).
  async function pauseMeeting() {
    try {
      // The chip turns into Resume: that says it.
      await invoke('meeting_pause');
    } catch (e) {
      error = String(e);
      toast({ title: "Couldn't stop recording", description: String(e), variant: 'destructive' });
    }
  }
  // The end-of-meeting moment: stop; once the recorder has transcribed the
  // last chunk, Rust writes the action plan (or, after only a few words,
  // asks "Started by mistake?").
  async function endMeeting() {
    flushSave();
    try {
      // The summary's runline ("Writing your action plan…") says it.
      await invoke('meeting_end', { origin: LABEL });
    } catch (e) {
      error = String(e);
      toast({ title: "Couldn't stop recording", description: String(e), variant: 'destructive' });
    }
  }

  // Write the action plan (again): the "Action plan" button, and the Action
  // Plan action on a meeting note. Rust runs it (meeting_end.rs), so the
  // meeting notepad shows its progress too.
  async function summariseMeeting() {
    if (!selected) return;
    // A silent recording (muted mic, nobody talking) with no typed notes
    // has nothing for the model to work with.
    if (!selected.content?.trim() && !selected.transcript?.some((s) => !s.echo && !s.dictated)) {
      // The answer to the click: the feedback lane, never queued.
      toast({
        kind: 'done',
        title: 'Nothing to summarise yet',
        description: 'No speech was transcribed in this recording.',
        chip: 'Tip',
        duration: 5000,
      });
      return;
    }
    flushSave();
    error = null;
    if (aiMissing?.noteId === selected.id) aiMissing = null;
    try {
      await summarise(selected.id);
    } catch (e) {
      toast({ title: "Couldn't write the action plan", description: String(e), variant: 'destructive' });
    }
  }

  function onMeetingState(s) {
    const wasRecording = meeting.recording;
    const noteId = meeting.noteId;
    meeting = s || { recording: false };
    if (meeting.recording) {
      startElapsed(meeting.elapsedSecs || 0);
    } else {
      stopElapsed();
      // Recording just finished → reload the note (transcript persisted).
      // The action plan (unless it was a pause) is Rust's: onSummary.
      if (wasRecording && noteId) {
        (async () => {
          if (selected?.id === noteId) await reloadSelected();
          refreshList();
        })();
      }
    }
  }

  // What the events below changed on the open note, field by field: a reload
  // keeps the fields an event changed while its read was out (a reply read
  // before the notepad's edit and delivered after it would put the old text
  // back, and the next keystroke here would save it). Notepad.svelte does
  // the same.
  const changed = {};
  function changing(...fields) {
    for (const f of fields) changed[f] = (changed[f] || 0) + 1;
  }
  let reloads = 0;

  /** The open note as stored, keeping what's being typed and what arrived meanwhile. */
  async function reloadSelected() {
    if (!selected) return;
    const id = selected.id;
    const asked = ++reloads;
    const before = { ...changed };
    try {
      const n = await invoke('note_get', { id });
      // Another note, or a newer reload on its way: that one says it.
      if (selected?.id !== id || asked !== reloads) return;
      // Keep what's being typed (its save is on the way).
      if (dirty.title) n.title = selected.title;
      if (dirty.content) n.content = selected.content;
      for (const f of Object.keys(changed)) if (changed[f] !== before[f]) n[f] = selected[f];
      selected = n;
    } catch {
      /* keep current */
    }
  }

  // The action plan Rust writes (meeting_end.rs): bring it in when done.
  async function onSummary(s) {
    if (!s) return;
    if (s.state !== 'running') refreshList();
    if (selected?.id !== s.noteId) return;
    if (s.state === 'done') {
      await reloadSelected();
      if (selected?.id === s.noteId && selected.enhancedContent) {
        tab = 'enhanced'; // OpenWhispr auto-switches to the Enhanced tab: that says it
      }
    } else if (s.state === 'error') {
      toast({ title: "Couldn't write the action plan", description: s.error, variant: 'destructive' });
    } else if (s.state === 'nothing') {
      toast({
        kind: 'info',
        title: 'Nothing to summarise yet',
        description: 'No speech was transcribed in this recording.',
        chip: 'Tip',
      });
    }
  }

  // The meeting notepad (or the AI meeting title) changed this note.
  function onNoteChanged(p) {
    if (!p || p.origin === LABEL) return;
    // The speaker engine's labels (every few seconds while a meeting
    // records) change only the lines and the map: no list refresh, and
    // nothing that's being typed is touched.
    const labels = p.origin === 'speakers';
    if (!labels) refreshList();
    if (selected?.id !== p.id) return;
    if (!labels) {
      changing('title', 'titleAuto', 'content', 'participants', 'folder');
      if (!dirty.title) {
        selected.title = p.title;
        selected.titleAuto = p.titleAuto;
      }
      if (!dirty.content) selected.content = p.content;
      selected.participants = p.participants;
      selected.folder = p.folder;
    }
    if (p.speakers) {
      changing('speakers');
      selected.speakers = p.speakers;
    }
    if (typeof p.enhancedContent === 'string') {
      changing('enhancedContent');
      selected.enhancedContent = p.enhancedContent;
    }
    // The voice separation labelled lines (`lineSplice`, some split where
    // the speaker changed), or a speaker was renamed or merged in the
    // notepad (`lineSpeakers`): every line's voice.
    if (p.lineSplice || p.lineSpeakers) {
      const t = p.lineSplice
        ? withLineSplice(selected.transcript, p)
        : withLineSpeakers(selected.transcript, p.lineSpeakers);
      if (t) {
        changing('transcript', 'breaks');
        selected.transcript = t;
        if (p.breaks) selected.breaks = p.breaks;
      } else {
        const id = selected.id;
        invoke('note_get', { id })
          .then((n) => {
            if (selected?.id === id) {
              selected.transcript = n.transcript;
              selected.speakers = n.speakers;
              selected.breaks = n.breaks;
            }
          })
          .catch(() => {});
      }
    }
  }

  // The meeting notepad, on this note (it opens on its own when a meeting
  // starts; this brings it back).
  function openNotepad() {
    if (!selected) return;
    flushSave();
    invoke('notepad_open', { noteId: selected.id }).catch((e) =>
      toast({ title: "Couldn't open the notepad", description: String(e), variant: 'destructive' })
    );
  }

  function onMeetingSegment(seg) {
    if (!seg) return;
    // Live-append to the open note's transcript view.
    if (selected && meeting.recording && meeting.noteId === selected.id && !hasLine(selected.transcript, seg)) {
      changing('transcript', 'noteType');
      selected.transcript = [...(selected.transcript || []), seg];
      selected.noteType = 'meeting';
    }
  }

  function onMeetingDigest(p) {
    if (p?.digest && selected?.id === p.noteId) {
      changing('digests');
      selected.digests = [...(selected.digests || []), p.digest];
    }
  }

  // Errors that mean "no AI model to summarise with" rather than a failure.
  function needsAiSetup(msg) {
    return /No AI model configured|API key/i.test(msg);
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
      loadSummary(id);
      tab = 'raw';
      transcriptView = 'live';
      showEcho = false;
      error = null;
      chatThread = []; // the embedded chat is pinned to one note
      chatInput = '';
      stopReading('notes:');
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

  // Delete a note. A meeting note goes the notepad's way (`meeting_delete`:
  // a recording into it stops first, the notepad lets go of it, its summary
  // job is dropped); anything else is `note_delete`, which refuses a note
  // that's recording.
  async function removeNote(id) {
    const n = notes.find((x) => x.id === id);
    const meetingNote = n?.noteType === 'meeting' || (meeting.recording && meeting.noteId === id);
    // What's being typed is saved first: kept if the delete fails.
    if (selected?.id === id) flushSave();
    try {
      await invoke(meetingNote ? 'meeting_delete' : 'note_delete', meetingNote ? { noteId: id } : { id });
    } catch (e) {
      toast({ title: "Couldn't delete the note", description: String(e), variant: 'destructive' });
      refreshList();
      return false;
    }
    if (selected?.id === id) selected = null;
    refreshList();
    return true;
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
    if (e.type === 'contextmenu' && !devInspect(e)) activeFolder = f;
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
      folders = await invoke('notes_folder_rename', { from, to });
      if (activeFolder === from) activeFolder = to;
      if (selected?.folder === from) selected.folder = to;
      await refreshList();
      setTimeout(() => folderEls[to]?.focus({ preventScroll: true }), 0);
    } catch (e) {
      toast({ title: "Couldn't rename the folder", description: String(e), variant: 'destructive' });
    }
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
      body:
        count === 0
          ? 'The folder is empty.'
          : `${count === 1 ? 'Its note' : `Its ${count} notes`} will move to Personal.`,
      run: () => deleteFolder(f),
      returnFocus: folderEls[f],
    });
  }
  async function deleteFolder(f) {
    try {
      folders = await invoke('notes_folder_delete', { name: f });
      if (activeFolder === f) activeFolder = 'Personal';
      if (selected?.folder === f) selected.folder = 'Personal';
      await refreshList();
      setTimeout(() => folderEls.Personal?.focus({ preventScroll: true }), 0);
      return true;
    } catch (e) {
      toast({ title: "Couldn't delete the folder", description: String(e), variant: 'destructive' });
      return false;
    }
  }
  // Notes: Rename (the title), Move to ▸, Copy as Markdown, Export…, Delete….
  let itemEls = $state({});
  function noteMenu(e, n) {
    const here = n.folder || 'Personal';
    const recording = meeting.recording && meeting.noteId === n.id;
    // A right-click opens the note too, as a click does — unless that would
    // throw away a question being asked about the open one.
    if (
      e.type === 'contextmenu' &&
      !devInspect(e) &&
      selected?.id !== n.id &&
      !chatInput.trim() &&
      !chatThread.length &&
      !asking
    ) {
      select(n.id);
    }
    openMenu(
      e,
      [
        { label: 'Rename', icon: 'pencil', onselect: () => renameNote(n.id) },
        {
          label: 'Move to',
          icon: 'folder',
          items: folders.map((f) => ({
            label: f,
            checked: f === here,
            disabled: f === here,
            onselect: () => moveToFolder(f, n.id),
          })),
        },
        { separator: true },
        readingNote(n.id)
          ? { label: 'Stop reading', icon: 'stop', onselect: () => readNote(n.id) }
          : { label: 'Read aloud', icon: 'play', onselect: () => readNote(n.id) },
        { label: 'Copy as Markdown', icon: 'copy', onselect: () => copyNoteMarkdown(n.id) },
        { label: 'Export…', icon: 'download', onselect: () => exportNote(n.id) },
        // A meeting kept on disk (meeting_audio.rs): on the speech model in use now.
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
              run: () => removeNote(n.id),
              returnFocus: itemEls[n.id],
            }),
        },
      ],
      `Note ${displayTitle(n)}`,
      itemEls[n.id]
    );
  }
  // The open note's audio kept on disk (meeting_audio.rs): ▶ on its lines,
  // what a crash left of it; kept current from `yap-meeting-audio`.
  let audioFor = null;
  $effect(() => {
    const id = selected?.id ?? null;
    if (id === audioFor) return;
    audioFor = id;
    stopLine();
    if (id != null) refreshAudio(id);
  });

  // "Transcribe again…" (the note's menu): asked first (meetingAudio.svelte.js).
  async function transcribeAgain(n) {
    if (selected?.id === n.id) flushSave();
    await refreshAudio(n.id);
    askTranscribeAgain(n, { title: displayTitle(n), returnFocus: itemEls[n.id] });
  }

  let titleEl = $state(null);
  async function renameNote(id) {
    if (selected?.id !== id) await select(id);
    if (selected?.id !== id) return;
    setTimeout(() => {
      titleEl?.focus();
      titleEl?.select();
    }, 0);
  }
  // Read a note aloud (read_aloud.rs: the reading card on the Yap bar, its
  // words lit up as they're said; it reads on if you leave Notes): the open
  // note as it's shown — the Enhanced summary on that tab, else the note —
  // or note `id` (its row's menu). Again while it reads: Stop.
  const readingNote = (id) => speech.speakingId === `note:${id}`;
  async function readNote(id = selected?.id) {
    if (id == null) return;
    if (selected?.id === id) flushSave();
    try {
      const n = selected?.id === id ? selected : await invoke('note_get', { id });
      const body = selected?.id === id && tab === 'enhanced' && n.enhancedContent ? n.enhancedContent : n.content || n.enhancedContent || '';
      const title = displayTitle(n);
      await readAloud(title && body.trim() ? `# ${title}\n\n${body}` : body, `note:${id}`);
    } catch (e) {
      toast({ title: "Couldn't read it aloud", description: String(e), variant: 'destructive' });
    }
  }

  async function copyNoteMarkdown(id) {
    if (selected?.id === id) flushSave();
    try {
      const n = selected?.id === id ? selected : await invoke('note_get', { id });
      await navigator.clipboard.writeText(noteMarkdown(n));
      toast({ title: 'Copied as markdown', variant: 'success' });
    } catch (e) {
      toast({ title: "Couldn't copy", description: String(e), variant: 'destructive' });
    }
  }

  // Debounced autosave of title + content while typing. flushSave() only
  // sends a pending edit (an armed timer) — switching notes or leaving Notes
  // must not re-save a note that was merely viewed, which would also clobber
  // edits made to it meanwhile through the local API. And only the fields
  // that were edited: the meeting notepad (or the AI title) may have changed
  // the other one meanwhile.
  let dirty = { title: false, content: false };
  function queueSave(field = 'content') {
    dirty[field] = true;
    clearTimeout(saveTimer);
    saveTimer = setTimeout(flushSave, 600);
  }
  function flushSave() {
    if (saveTimer === null) return; // nothing typed since the last save
    clearTimeout(saveTimer);
    saveTimer = null;
    if (!selected) return;
    const args = { id: selected.id, origin: LABEL };
    if (dirty.title) args.title = selected.title;
    if (dirty.content) args.content = selected.content;
    dirty = { title: false, content: false };
    invoke('note_update', args)
      .then(refreshList)
      .catch(() => {});
  }

  const isStale = $derived.by(() => {
    if (!selected || !selected.enhancedContent) return false;
    const s = notes.find((n) => n.id === selected.id);
    return !!s?.stale;
  });

  async function runAction(action) {
    if (!selected || enhancing.has(selected.id) || jobRunning || !action) return;
    pickerOpen = false;
    lastUsedActionId = action.id;
    localStorage.setItem('yapLastUsedActionId', String(action.id));
    // A meeting's action plan is Rust's job (progress shows in the notepad
    // too); any other action runs here.
    if (action.kind === 'actionPlan' && selected.transcript?.length) {
      summariseMeeting();
      return;
    }
    flushSave();
    const id = selected.id;
    enhancing = new Set([...enhancing, id]);
    runningAction = { noteId: id, name: action.name };
    error = null;
    if (aiMissing?.noteId === id) aiMissing = null;
    try {
      const enhanced = await invoke('note_enhance', { id, actionId: action.id });
      // Only mutate the open note if the user is still on it.
      if (selected?.id === id) {
        selected.enhancedContent = enhanced;
        tab = 'enhanced'; // OpenWhispr auto-switches to the Enhanced tab
        try {
          selected.digests = (await invoke('note_get', { id })).digests;
        } catch {
          /* keep current */
        }
      }
      // The Enhanced tab, opened on the result, says it (no toast on top).
      refreshList();
    } catch (e) {
      const msg = String(e);
      if (needsAiSetup(msg)) {
        // Not a failure: the note is saved, it just needs a model. The card
        // in the editor says so (and links to Settings).
        aiMissing = { noteId: id, message: msg };
      } else {
        if (selected?.id === id) error = msg;
        toast({ title: `${action.name} failed`, description: msg, variant: 'destructive' });
      }
    } finally {
      const next = new Set(enhancing);
      next.delete(id);
      enhancing = next;
      if (runningAction?.noteId === id) runningAction = null;
    }
  }
  // `{ noteId, name }` of the action running, for the status line.
  let runningAction = $state(null);

  // Copy the open tab: markdown as is, or as plain text (headings and
  // checkboxes spelled out) for chat apps and email.
  async function copyEnhanced(asText = false) {
    let text = tab === 'enhanced' ? selected?.enhancedContent : selected?.content;
    if (!text) return;
    if (asText) text = markdownToText(text);
    try {
      await navigator.clipboard.writeText(text);
      copied = asText ? 'text' : 'markdown';
      setTimeout(() => (copied = false), 1500);
      toast({ title: asText ? 'Copied as text' : 'Copied as markdown', variant: 'success' });
    } catch {
      /* clipboard unavailable */
    }
  }

  // ---- Editor meta chips (OpenWhispr NoteEditor header) ----
  let addingAttendee = $state(false);
  let attendeeInput = $state('');
  let attendeeEl = $state(null);
  let folderMenuOpen = $state(false);
  let addingFolderInMenu = $state(false);

  async function createFolderAndMove() {
    const name = newFolderName.trim();
    addingFolderInMenu = false;
    newFolderName = '';
    if (!name) return;
    try {
      folders = await invoke('notes_folder_create', { name });
      await moveToFolder(name);
    } catch (e) {
      toast({ title: "Couldn't create folder", description: String(e), variant: 'destructive' });
    }
  }

  function persistParticipants() {
    if (!selected) return;
    invoke('note_update', { id: selected.id, participants: selected.participants || [] }).catch(
      () => {}
    );
  }
  function addAttendee() {
    const name = attendeeInput.trim();
    attendeeInput = '';
    if (!name || !selected) return;
    // Popover stays open so several attendees can be added in a row.
    selected.participants = [...(selected.participants || []), name];
    persistParticipants();
  }
  function removeAttendee(i) {
    if (!selected) return;
    selected.participants = (selected.participants || []).filter((_, j) => j !== i);
    persistParticipants();
  }
  // Move the open note (the folder chip), or note `id` (its row's menu).
  async function moveToFolder(f, id = selected?.id) {
    folderMenuOpen = false;
    if (id == null) return;
    const open = selected?.id === id ? selected : null;
    const from = open ? open.folder : notes.find((n) => n.id === id)?.folder;
    if ((from || 'Personal') === f) return;
    if (open) {
      changing('folder');
      open.folder = f;
    }
    try {
      await invoke('note_update', { id, folder: f });
      refreshList();
      toast({ title: `Moved to ${f}`, variant: "success" });
    } catch (e) {
      toast({ title: "Couldn't move note", description: String(e), variant: 'destructive' });
    }
  }
  // Recording from a note makes it a meeting note, filed in Meetings like
  // every note "Take notes" makes, unless the person chose its folder
  // (notes::file_as_meeting). The open note's list follows it there, and a
  // notice says so, with the way back (a move that then sticks).
  function onNoteFiled(p) {
    if (p?.noteId == null || !p.to) return;
    const open = selected?.id === p.noteId ? selected : null;
    if (open) {
      changing('folder', 'noteType');
      open.folder = p.to;
      open.noteType = 'meeting';
      if (activeFolder === p.from) activeFolder = p.to;
    }
    refreshList();
    if (!open) return;
    // The answer to the click that started the recording (the feedback
    // lane): never queued in front of what the meeting itself says, like
    // its length warning.
    toast({
      kind: 'done',
      key: `note-filed-${p.noteId}`,
      chip: 'Notes',
      title: `Moved to ${p.to}`,
      description: `“${displayTitle(open)}” is a meeting note now, so it's with your other meetings.`,
      duration: 10_000,
      action: {
        label: `Keep in ${p.from}`,
        onClick: async () => {
          await moveToFolder(p.from, p.noteId);
          if (selected?.id === p.noteId && activeFolder === p.to) activeFolder = p.from;
        },
      },
    });
  }

  // Export the open note (the editor's button), or note `id` (its row's menu).
  async function exportNote(id = selected?.id) {
    if (id == null) return;
    if (selected?.id === id) flushSave();
    try {
      const n = selected?.id === id ? selected : notes.find((x) => x.id === id);
      const { save } = await import('@tauri-apps/plugin-dialog');
      const suggested = (displayTitle(n || {}) || 'note').replace(/[\\/:*?"<>|]/g, '-');
      const path = await save({
        defaultPath: `${suggested}.md`,
        filters: [{ name: 'Markdown', extensions: ['md'] }],
      });
      if (!path) return;
      await invoke('note_export', { id, path });
      toast({ title: 'Note exported', description: path, variant: 'success' });
    } catch (e) {
      toast({ title: 'Export failed', description: String(e), variant: 'destructive' });
    }
  }

  function chipDate(ts) {
    return new Date((ts || 0) * 1000).toLocaleDateString(undefined, {
      day: 'numeric',
      month: 'short',
    });
  }
  // Relative time for list rows (OpenWhispr's "now" badge style).
  function relTime(ts) {
    const diff = Math.floor(Date.now() / 1000) - ts;
    if (diff < 60) return 'now';
    if (diff < 3600) return `${Math.floor(diff / 60)}m`;
    const d = new Date(ts * 1000);
    if (d.toDateString() === new Date().toDateString()) {
      return d.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' });
    }
    return chipDate(ts);
  }
  const folderCounts = $derived.by(() => {
    const counts = {};
    for (const n of notes) counts[n.folder || 'Personal'] = (counts[n.folder || 'Personal'] || 0) + 1;
    return counts;
  });

  // ---- Embedded note chat (the "Ask anything…" bar, useEmbeddedChat port) ----
  // { role: 'user'|'assistant', text, model? } — `model`: who answered
  // (typed questions go to the Chat scope, the helpers to Note Formatting).
  let chatThread = $state([]);
  let chatInput = $state('');
  let asking = $state(false);
  // Each answer's id for ▶ Read aloud (speech.svelte.js / tts.rs), and a
  // note's own ("note:<id>").
  initSpeech(['notes:', 'note:']);
  let turnSeq = 0;
  const turnId = () => `notes:${selected?.id ?? 0}:${++turnSeq}`;

  async function askNote() {
    const q = chatInput.trim();
    if (!q || !selected || asking) return;
    chatInput = '';
    flushSave();
    const history = chatThread.slice(-6).map((t) => [t.role, t.text]);
    chatThread = [...chatThread, { role: 'user', text: q }];
    asking = true;
    try {
      const [answer, model] = await Promise.all([
        invoke('note_ask', { id: selected.id, question: q, history }),
        nextModel('chat'),
      ]);
      chatThread = [...chatThread, { role: 'assistant', text: answer, model, id: turnId() }];
    } catch (e) {
      chatThread = chatThread.slice(0, -1);
      chatInput = q; // give the question back
      toast({ title: "Couldn't answer", description: String(e), variant: 'destructive' });
    } finally {
      asking = false;
    }
  }

  // A meeting note's one-tap helpers (meeting_assist.rs), as chips over the
  // bar: Questions to ask while it records, Follow-up email and Gaps and
  // risks after. ("What did I miss?" stays the notepad's: it knows what you
  // have seen.)
  const noteHelpers = $derived(
    selected?.noteType === 'meeting'
      ? meetingHelpers(selected, recordingThisNote).filter((k) => k !== 'catchUp')
      : []
  );
  async function askHelper(kind) {
    if (!selected || asking) return;
    const id = selected.id;
    flushSave();
    chatThread = [...chatThread, { role: 'user', text: MEETING_HELPERS[kind].label }];
    asking = true;
    try {
      const [r, model] = await Promise.all([invoke('meeting_assist', { noteId: id, kind }), nextModel('notes')]);
      if (selected?.id !== id) return;
      // "Too little to go on" comes from Yap, not a model.
      chatThread = [
        ...chatThread,
        { role: 'assistant', text: r.answer, plain: !!r.plain, copy: true, model: r.tooLittle ? null : model, id: turnId() },
      ];
    } catch (e) {
      if (selected?.id !== id) return;
      chatThread = chatThread.slice(0, -1);
      const msg = String(e);
      const setup = /No AI model configured|API key/i.test(msg) && onopensettings;
      toast({
        title: "Couldn't answer",
        description: msg,
        variant: 'destructive',
        ...(setup ? { action: { label: 'Open Language Models', onClick: () => onopensettings('cleanup') } } : {}),
      });
    } finally {
      asking = false;
    }
  }
  let copiedTurn = $state(-1);
  function turnMenu(e, turn) {
    const reading = !!turn.id && speech.speakingId === turn.id;
    openContextMenu(
      e,
      messageItems(turn, { read: turn.role === 'assistant' && turn.id ? () => readAloud(turn.text, turn.id) : null, reading }),
      { label: turn.role === 'assistant' ? 'Answer' : 'Question', row: e.currentTarget }
    );
  }

  async function copyTurn(turn, i) {
    try {
      await navigator.clipboard.writeText(turn.text);
      copiedTurn = i;
      setTimeout(() => copiedTurn === i && (copiedTurn = -1), 1200);
    } catch {
      toast({ title: "Couldn't copy", description: 'The clipboard is busy. Try again.', variant: 'destructive' });
    }
  }

  // The bar's mic: focus the input and start dictation — Yap types wherever
  // the caret is, so the answer box IS the dictation target.
  let askEl = $state(null);
  function micAsk() {
    askEl?.focus();
    invoke('toggle_recording').catch(() => {});
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
    const unlisteners = [];
    // A meeting transcribed again (or what a crash left): its new lines.
    initMeetingAudio();
    unlisteners.push(onJobDone((job) => job.noteId === selected?.id && reloadSelected()));
    invoke('meeting_state')
      .then((s) => {
        meeting = s || { recording: false };
        if (meeting.recording) startElapsed(meeting.elapsedSecs || 0);
      })
      .catch(() => {});
    listen('yap-meeting-state', (e) => onMeetingState(e.payload)).then((u) =>
      unlisteners.push(u)
    );
    listen('yap-meeting-segment', (e) => onMeetingSegment(e.payload)).then((u) =>
      unlisteners.push(u)
    );
    listen('yap-meeting-digest', (e) => onMeetingDigest(e.payload)).then((u) =>
      unlisteners.push(u)
    );
    listen('yap-meeting-summary', (e) => onSummary(e.payload)).then((u) => unlisteners.push(u));
    listen('yap-note-changed', (e) => onNoteChanged(e.payload)).then((u) => unlisteners.push(u));
    // (A recording's warnings are notices from meetingGuard.js, in any view.)
    // The local API bridge (Integrations) can create/update/delete notes from
    // outside the app — refresh the list so external edits show up live.
    listen('yap-notes-changed', () => refreshList()).then((u) => unlisteners.push(u));
    // A recording made a note a meeting note and filed it in Meetings
    // (commands.rs `meeting_start` → notes::file_as_meeting).
    listen('yap-note-filed', (e) => onNoteFiled(e.payload)).then((u) => unlisteners.push(u));
    // "Started by mistake?" → Discard (here or in the notepad).
    listen('yap-note-deleted', (e) => {
      if (selected?.id === e.payload?.id) selected = null;
      refreshList();
    }).then((u) => unlisteners.push(u));
    return () => {
      flushSave();
      stopElapsed();
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
    openRequested(req);
  });

  async function openRequested({ id, stop }) {
    await select(id);
    if (selected?.id !== id) return;
    activeFolder = selected.folder || activeFolder;
    refreshList();
    if (!stop) return;
    meeting = (await invoke('meeting_state').catch(() => null)) || { recording: false };
    if (meeting.recording && meeting.noteId === id) await endMeeting();
  }
</script>

<svelte:window
  onclick={() => {
    pickerOpen = false;
    folderMenuOpen = false;
    addingAttendee = false;
    addingFolderInMenu = false;
  }}
/>

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
              class:active={activeFolder === f}
              aria-haspopup="menu"
              onclick={() => (activeFolder = f)}
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
    <div class="seccap">
      <span>Notes</span>
      <button class="secadd" title="New note" aria-label="New note" onclick={newNote}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
      </button>
    </div>
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
            bind:this={itemEls[n.id]}
            class="item"
            class:active={selected?.id === n.id}
            class:menuopen={menuOpenFor(itemEls[n.id])}
            role="button"
            tabindex="0"
            aria-haspopup="menu"
            onclick={() => select(n.id)}
            oncontextmenu={(e) => noteMenu(e, n)}
            onkeydown={(e) => {
              if (e.target !== e.currentTarget) return;
              if (e.key === 'Enter' || e.key === ' ') select(n.id);
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
          bind:this={titleEl}
          class="title"
          class:auto={selected.titleAuto}
          placeholder="Untitled Note"
          bind:value={selected.title}
          oninput={() => queueSave('title')}
        />
      </div>

      <!-- Meta chip row (OpenWhispr NoteEditor header): date · attendees ·
           folder, with export at the right. -->
      <div class="metarow">
        <span class="chip static">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3" y="4" width="18" height="18" rx="2" /><path d="M16 2v4M8 2v4M3 10h18" /></svg>
          {chipDate(selected.createdTs)}
        </span>
        {#each selected.participants || [] as p, i (p + i)}
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
              folderMenuOpen = false;
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
              folderMenuOpen = !folderMenuOpen;
              addingAttendee = false;
              addingFolderInMenu = false;
            }}
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 20h16a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.7-.9L9.2 3.9A2 2 0 0 0 7.5 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2z" /></svg>
            {selected.folder || 'Personal'}
          </button>
          {#if folderMenuOpen}
            <div class="chipmenu" role="presentation" onclick={(e) => e.stopPropagation()}>
              {#each folders as f (f)}
                <button class="chipmenuitem" class:current={f === selected.folder} role="menuitem" onclick={() => moveToFolder(f)}>
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 20h16a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.7-.9L9.2 3.9A2 2 0 0 0 7.5 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2z" /></svg>
                  <span class="cmname">{f}</span>
                  {#if f === selected.folder}
                    <svg class="cmcheck" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 12l5 5L20 6" /></svg>
                  {/if}
                </button>
              {/each}
              <div class="msep"></div>
              {#if addingFolderInMenu}
                <input
                  class="popinput"
                  placeholder="Folder name…"
                  bind:value={newFolderName}
                  onkeydown={(e) => {
                    if (e.key === 'Enter') createFolderAndMove();
                    if (e.key === 'Escape') {
                      addingFolderInMenu = false;
                      newFolderName = '';
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
        {#if selected.noteType === 'meeting' || recordingThisNote}
          <button
            class="chip"
            onclick={openNotepad}
            title="Open the meeting notepad beside your call"
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3" y="3" width="18" height="18" rx="2" /><path d="M15 3v18" /><path d="M18 8h0M18 12h0" /></svg>
            Notepad
          </button>
        {/if}
        <!-- Meeting recorder: mic ("You") + system audio ("Them"). Pause
             keeps the note as it is; "End meeting & summarise" (bottom bar)
             ends it, and Rust writes the action plan. -->
        {#if recordingThisNote}
          <button
            class="chip reclive"
            onclick={pauseMeeting}
            aria-label="Pause recording"
            title="Pause — Resume carries on in this note"
          >
            <span class="recdot"></span>
            {fmtElapsed(elapsed)} · Pause
          </button>
        {:else}
          <button
            class="chip"
            onclick={startMeeting}
            disabled={meeting.recording}
            title={meeting.recording
              ? 'Already recording another note'
              : 'Record this meeting — your mic is "You", the call audio is "Them"'}
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="2" width="6" height="12" rx="3" /><path d="M5 10v1a7 7 0 0 0 14 0v-1" /><path d="M12 18v4" /></svg>
            {selected.transcript?.length ? 'Resume' : 'Record'}
          </button>
        {/if}
        <!-- Read aloud (read_aloud.rs): the reading card on the Yap bar. -->
        <button
          class="iconbtn"
          class:on={readingNote(selected.id)}
          title={readingNote(selected.id) ? 'Stop reading' : 'Read aloud'}
          aria-label={readingNote(selected.id) ? 'Stop reading' : 'Read aloud'}
          aria-pressed={readingNote(selected.id)}
          onclick={() => readNote()}
        >
          {#if readingNote(selected.id)}
            <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><rect x="7" y="7" width="10" height="10" rx="2" /></svg>
          {:else}
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 9.5v5h3.5L12 18.5v-13L7.5 9.5z" /><path d="M15.5 9a4 4 0 0 1 0 6" /><path d="M18.5 6.5a7.5 7.5 0 0 1 0 11" /></svg>
          {/if}
        </button>
        <button class="iconbtn" title="Export as Markdown" aria-label="Export as Markdown" onclick={() => exportNote()}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" /><path d="M7 10l5 5 5-5" /><path d="M12 15V3" /></svg>
        </button>
      </div>

      {#if selected.enhancedContent}
        <div class="tabs">
          <!-- The meeting notepad's names for the same two things. -->
          <button class="tab" class:active={tab === 'raw'} onclick={() => (tab = 'raw')}>My thoughts</button>
          <button class="tab" class:active={tab === 'enhanced'} onclick={() => (tab = 'enhanced')}>
            Summary
            {#if isStale}<span class="staledot" title="Your notes changed since this summary: run the action again to update it"></span>{/if}
          </button>
          <span class="tabspacer"></span>
          <button class="copy" onclick={() => copyEnhanced(false)}>
            {copied === 'markdown' ? 'Copied!' : 'Copy as Markdown'}
          </button>
          <button class="copy" onclick={() => copyEnhanced(true)}>
            {copied === 'text' ? 'Copied!' : 'Copy as text'}
          </button>
        </div>
      {/if}

      {#if runningAction?.noteId === selected.id || jobRunning}
        <p class="runline" aria-live="polite">
          <span class="spin"></span>
          {#if summaryProgress?.total > 0}
            Catching up on the meeting — part {summaryProgress.done + 1} of {summaryProgress.total}…
          {:else if jobRunning || runningAction?.name === actionPlanAction?.name}
            Writing your action plan…
          {:else}
            Running {runningAction.name}…
          {/if}
        </p>
      {/if}

      {#if needsAi}
        <div class="aicard">
          <div class="aitext">
            <strong>Your meeting is saved.</strong>
            To turn it into an action plan, Yap needs an AI model: set one up in Settings → Language
            Models (an on-device model keeps everything on this PC), then press Action plan.
            <span class="aidetail">{needsAi}</span>
          </div>
          <button class="aibtn" onclick={() => onopensettings?.('cleanup')}>Open Language Models</button>
        </div>
      {/if}

      {#if error}
        <p class="errline">{error}</p>
      {:else if job?.state === 'error'}
        <p class="errline">{job.error}</p>
      {/if}

      {#if tab === 'enhanced' && selected.enhancedContent}
        <!-- eslint-disable-next-line svelte/no-at-html-tags — renderMarkdown
             passes no raw HTML and sanitises its output (lib/markdown.js) -->
        <div class="rendered">{@html renderMarkdown(selected.enhancedContent)}</div>
      {:else}
        <textarea
          class="raw"
          placeholder={recordingThisNote
            ? 'Take rough notes while Yap listens — they get woven into the minutes…'
            : 'Start writing…'}
          bind:value={selected.content}
          oninput={() => queueSave('content')}
        ></textarea>
      {/if}

      <!-- Meeting transcript (You/Them chat bubbles, OpenWhispr
           MeetingTranscriptChat style). Live while recording; kept afterwards. -->
      {#if recordingThisNote || selected.transcript?.length}
        <div class="transcript">
          <div class="thead">
            {#if selected.digests?.length}
              <!-- Transcript ↔ the rolling AI notes (meeting_summary.rs) -->
              <span class="tviews">
                <button class="tview" class:on={transcriptView === 'live'} onclick={() => (transcriptView = 'live')}>
                  Transcript
                </button>
                <button class="tview" class:on={transcriptView === 'notes'} onclick={() => (transcriptView = 'notes')}>
                  AI notes so far
                </button>
              </span>
            {:else}
              <span class="tcap">Meeting transcript</span>
            {/if}
            {#if recordingThisNote}
              <span class="tlive"><span class="recdot"></span> listening — transcribes every ~15 s</span>
            {/if}
            <span class="tspacer"></span>
            {#if digestedUpTo}
              <span
                class="tdigest"
                title="Yap summarises the meeting as it goes, so the action plan is ready in seconds however long it runs"
              >
                AI notes up to {digestedUpTo}
              </span>
            {/if}
            {#if !recordingThisNote && selected.transcript?.length}
              <button
                class="planbtn"
                onclick={summariseMeeting}
                disabled={enhancing.has(selected.id) || jobRunning || meeting.recording && meeting.noteId === selected.id}
                title="Who does what by when, with decisions and open questions"
              >
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 11l3 3 8-8" /><path d="M20 12v7a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h9" /></svg>
                Action plan
              </button>
            {/if}
          </div>
          {#if jobState(selected.id)}
            <!-- Its audio being transcribed (meeting_audio.rs), with Cancel; a refine's "Update summary". -->
            <MeetingJob noteId={selected.id} look="app" />
          {:else if meetingAudio.byNote[selected.id]?.recoverableSecs && !recordingThisNote}
            <!-- What a crash left of it: the audio is kept. -->
            <div class="trecover" role="status">
              <span>
                Yap closed while this meeting was recording. The last
                {lastPart(meetingAudio.byNote[selected.id].recoverableSecs)} transcribed; the audio is kept.
              </span>
              <button class="planbtn" onclick={() => recoverMeeting(selected.id)}>Transcribe it</button>
            </div>
          {/if}
          {#if selected.transcript?.length}
            <!-- "Speaker 2 is Priya?" (speakers/naming.rs) -->
            <SpeakerCard note={selected} origin={LABEL} onchanged={onSpeakerChanged} />
          {/if}
          {#if recordingThisNote && !selected.transcript?.length}
            <p class="thint">
              Your mic is <strong>You</strong>, the call audio is <strong>Them</strong>. Use
              headphones so your mic doesn't also hear the call.
            </p>
          {/if}
          {#if transcriptView === 'notes' && selected.digests?.length}
            <div class="tscroll" role="region" aria-label="AI notes so far">
              {#each selected.digests as d, i (i)}
                <div class="part">
                  <span class="partrange">{digestRange(d)}</span>
                  <ul>
                    {#each d.keyPoints || [] as p, j (j)}<li>{p}</li>{/each}
                    {#each d.decisions || [] as p, j (j)}<li><strong>Decided:</strong> {p}</li>{/each}
                    {#each d.actions || [] as a, j (j)}
                      <li class="ptask">☐ {a.owner}: {a.task}{a.due ? ` (due ${a.due})` : ''}</li>
                    {/each}
                  </ul>
                </div>
              {/each}
            </div>
          {:else}
            <div
              class="tscroll"
              role="log"
              aria-label="Meeting transcript"
              bind:this={logEl}
              onscroll={() =>
                (followLog = logEl.scrollHeight - logEl.scrollTop - logEl.clientHeight < 40)}
            >
              {#each transcriptRuns as run (run.key)}
                {#if run.kind === 'dictated'}
                  <div class="dictated" title="Yap kept what you dictated out of the meeting's transcript and summaries">
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="3" width="6" height="11" rx="3" /><path d="M5 11a7 7 0 0 0 14 0" /><path d="M12 18v3" /></svg>
                    <span>You dictated here · left out of the notes</span>
                  </div>
                {:else if run.kind === 'failed'}
                  <FailedChunk
                    noteId={selected.id}
                    seg={selected.transcript[run.i]}
                    why={recordingThisNote ? 'After the meeting' : meetingAudio.byNote[selected.id] && !meetingAudio.byNote[selected.id].recordings ? "This meeting's audio isn't kept" : null}
                  />
                {:else if run.kind === 'group'}
                  {#each run.lines as line, j (line.i)}
                    <!-- Focusable for its menu from the keyboard (the ContextMenu key, Shift+F10). -->
                    <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
                    <div
                      class="bubble {run.source}"
                      class:echo={run.echo}
                      class:cont={j > 0}
                      class:menuopen={menuOpenFor(lineEls[line.i]) || speakerMenu?.key === `l${line.i}`}
                      bind:this={lineEls[line.i]}
                      role="article"
                      tabindex="0"
                      aria-label={`${run.label}: ${line.text}`}
                      oncontextmenu={(e) => lineMenu(e, run, line)}
                      onkeydown={(e) => e.target === e.currentTarget && menuKey(e, (k) => lineMenu(k, run, line))}
                    >
                      {#if j === 0}
                        {#if run.echo}
                          <span class="who">{run.label} · from the speakers</span>
                        {:else}
                          <span class="spkwrap">
                            <button
                              class="who"
                              class:guess={run.guess}
                              aria-haspopup="menu"
                              aria-expanded={speakerMenu?.key === run.key}
                              title={run.guess ? 'A guess: click to confirm or change' : 'Rename, merge or correct this speaker'}
                              onclick={() => toggleSpeakerMenu(run.key, run.speaker, run.lines.map((l) => l.i))}
                            >{run.label}</button>
                            {#if speakerMenu?.key === run.key}
                              <SpeakerMenu
                                note={selected}
                                voice={speakerMenu.voice}
                                lines={speakerMenu.lines}
                                origin={LABEL}
                                align={run.source === 'you' ? 'right' : 'left'}
                                onclose={() => (speakerMenu = null)}
                                onchanged={onSpeakerChanged}
                              />
                            {/if}
                          </span>
                        {/if}
                      {/if}
                      <p>{line.text}</p>
                      {#if canPlay(selected.id, selected.transcript[line.i])}
                        <!-- ▶ this line's stretch of the meeting, both sides (meeting_audio.rs). -->
                        <button
                          class="lineplay"
                          class:on={meetingAudio.playing === `${selected.id}:${line.i}`}
                          aria-label={meetingAudio.playing === `${selected.id}:${line.i}` ? 'Stop playing' : 'Play this line'}
                          title={meetingAudio.playing === `${selected.id}:${line.i}` ? 'Stop' : 'Play this line'}
                          onclick={() => playLine(selected.id, line.i, selected.transcript[line.i])}
                        >
                          {#if meetingAudio.playing === `${selected.id}:${line.i}`}
                            <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="7" y="7" width="10" height="10" rx="1.5" fill="currentColor" /></svg>
                          {:else}
                            <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M8 5.5v13l10.5-6.5z" fill="currentColor" /></svg>
                          {/if}
                        </button>
                      {/if}
                      {#if speakerMenu?.key === `l${line.i}`}
                        <!-- "Who said this?" from the line's menu: the speaker menu at the line. -->
                        <SpeakerMenu
                          note={selected}
                          voice={speakerMenu.voice}
                          lines={speakerMenu.lines}
                          start="lines"
                          origin={LABEL}
                          align={run.source === 'you' ? 'right' : 'left'}
                          onclose={() => (speakerMenu = null)}
                          onchanged={onSpeakerChanged}
                        />
                      {/if}
                    </div>
                  {/each}
                {/if}
              {/each}
            </div>
            {#if echoCount}
              <button class="echonote" onclick={() => (showEcho = !showEcho)}>
                {showEcho ? 'Hide' : 'Show'}
                {echoCount}
                {echoCount === 1 ? 'line' : 'lines'} your mic picked up from the speakers (left out of
                summaries — headphones avoid this)
              </button>
            {/if}
          {/if}
        </div>
      {/if}

      <!-- Embedded note chat thread (answers grounded in this note). -->
      {#if chatThread.length > 0}
        <div class="chatthread">
          {#each chatThread as turn, i (i)}
            <!-- Its right-click menu, as Chat's: an answer's Copy / Copy as
                 Markdown / Read aloud, a question's Copy. Focusable for it
                 from the keyboard (the ContextMenu key, Shift+F10). -->
            <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
            <div
              class="cbubble {turn.role}"
              role="article"
              tabindex="0"
              aria-label={turn.role === 'assistant' ? 'Answer' : 'Your question'}
              oncontextmenu={(e) => turnMenu(e, turn)}
              onkeydown={(e) => e.target === e.currentTarget && menuKey(e, (k) => turnMenu(k, turn))}
            >
              {#if turn.role === 'assistant' && turn.plain}
                <!-- An email: shown exactly as it copies. -->
                <div class="cbody plain">{turn.text}</div>
              {:else if turn.role === 'assistant'}
                <!-- eslint-disable-next-line svelte/no-at-html-tags — renderMarkdown sanitises its output -->
                <div class="cbody">{@html renderMarkdown(turn.text)}</div>
              {:else}
                <p class="cbody">{turn.text}</p>
              {/if}
              {#if turn.copy}
                <button class="ccopy" onclick={() => copyTurn(turn, i)}>{copiedTurn === i ? 'Copied' : 'Copy'}</button>
              {/if}
            </div>
            {#if turn.role === 'assistant'}
              {@const reading = !!turn.id && speech.speakingId === turn.id}
              <div class="turnfoot">
                {#if turn.id}
                  <button
                    class="readbtn"
                    class:on={reading}
                    aria-label={reading ? 'Stop reading' : 'Read aloud'}
                    title={reading ? 'Stop reading' : 'Read aloud'}
                    onclick={() => readAloud(turn.text, turn.id)}
                  >
                    {#if reading}
                      <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><rect x="5" y="5" width="14" height="14" rx="2.5" /></svg>
                    {:else}
                      <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M7 4.9v14.2a1.2 1.2 0 0 0 1.8 1l11.6-7.1a1.2 1.2 0 0 0 0-2L8.8 3.9A1.2 1.2 0 0 0 7 4.9z" /></svg>
                    {/if}
                  </button>
                {/if}
                {#if reading}
                  <span class="speaking" role="status">Speaking…</span>
                {:else if turn.model}
                  <span class="byline" title={whereItRuns(turn.model)}>{modelLine(turn.model)}</span>
                {/if}
              </div>
            {/if}
          {/each}
          {#if asking}
            <div class="cbubble assistant"><p class="cbody thinking">Thinking…</p></div>
          {/if}
        </div>
      {/if}

      {#if noteHelpers.length && !chatInput.trim()}
        <div class="helperrow" role="group" aria-label="Meeting helpers">
          {#each noteHelpers as kind (kind)}
            <button class="helperchip" onclick={() => askHelper(kind)} disabled={asking}>{MEETING_HELPERS[kind].label}</button>
          {/each}
        </div>
      {/if}

      <!-- Bottom bar (OpenWhispr NoteEditor): mic · Ask anything… · Generate Notes -->
      <div class="askbar">
        <button
          class="micbtn"
          title="Dictate your question (starts recording into the box)"
          aria-label="Dictate your question"
          onclick={micAsk}
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="2" width="6" height="12" rx="3" /><path d="M5 10v1a7 7 0 0 0 14 0v-1" /><path d="M12 18v4" /></svg>
        </button>
        <input
          class="askinput"
          bind:this={askEl}
          bind:value={chatInput}
          placeholder="Ask anything…"
          aria-busy={asking}
          onkeydown={(e) => e.key === 'Enter' && !e.isComposing && askNote()}
        />
        <!-- Not `disabled` while asking: that would throw the caret out of
             the box (askNote ignores Enter until the answer is in). -->
        <!-- While recording: the end-of-meeting moment. Otherwise the
             ActionPicker split button (OpenWhispr ActionPicker.tsx): left half
             runs the last-used action, the chevron opens the action menu. -->
        {#if recordingThisNote}
          <button class="enhance endmeeting" onclick={endMeeting}>
            <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="6" y="6" width="12" height="12" rx="2" fill="currentColor" /></svg>
            End meeting &amp; summarise
          </button>
        {:else}
        <div class="picker">
          <button
            class="enhance runhalf"
            onclick={() => runAction(activeAction)}
            disabled={!activeAction ||
              enhancing.has(selected.id) ||
              jobRunning ||
              (!selected.content?.trim() && !selected.transcript?.length)}
            title={activeAction ? `Run "${activeAction.name}"` : 'No actions'}
          >
            {#if enhancing.has(selected.id) || jobRunning}
              <span class="spin"></span> Running…
            {:else}
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3v3M12 18v3M3 12h3M18 12h3M5.6 5.6l2.1 2.1M16.3 16.3l2.1 2.1M18.4 5.6l-2.1 2.1M7.7 16.3l-2.1 2.1" /></svg>
              {activeAction?.name ?? 'Enhance'}
            {/if}
          </button>
          <button
            class="enhance chevron"
            aria-label="Select action"
            disabled={enhancing.has(selected.id) || jobRunning}
            onclick={(e) => {
              e.stopPropagation();
              pickerOpen = !pickerOpen;
            }}
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m6 9 6 6 6-6" /></svg>
          </button>
          {#if pickerOpen}
            <div class="menu up" role="menu">
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
        {/if}
      </div>
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
  /* A title Yap made up ("Teams call · 5 Oct, 14:30"), until the AI names
     the meeting or the person does. */
  .title.auto {
    color: var(--yap-muted);
    font-weight: 600;
  }
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

  /* meeting transcript */
  .transcript {
    flex: 0 0 auto;
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: 12px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
    padding: 10px 12px;
    max-height: 240px;
  }
  .thead {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 24px;
  }
  .tspacer {
    flex: 1;
  }
  .tviews {
    display: inline-flex;
    gap: 2px;
    padding: 2px;
    border-radius: var(--yap-r);
    background: var(--yap-s1);
    border: 1px solid var(--yap-border-subtle);
  }
  .tview {
    height: 22px;
    padding: 0 9px;
    border: none;
    border-radius: var(--yap-r-sm);
    background: none;
    color: var(--yap-muted);
    font: inherit;
    font-size: 10.5px;
    font-weight: 700;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    cursor: pointer;
  }
  .tview.on {
    background: var(--yap-primary-wash);
    color: var(--yap-fg);
  }
  .tdigest {
    font-size: 11px;
    color: var(--yap-muted);
    white-space: nowrap;
  }
  .planbtn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 26px;
    padding: 0 11px;
    border: none;
    border-radius: var(--yap-r);
    background: var(--yap-ink, var(--yap-primary));
    color: var(--yap-ink-fg, var(--yap-primary-fg));
    font: inherit;
    font-size: 11.5px;
    font-weight: 600;
    cursor: pointer;
    white-space: nowrap;
  }
  .planbtn:hover:not(:disabled) {
    background: var(--yap-ink-hover, var(--yap-primary-hover));
  }
  .planbtn:disabled {
    opacity: 0.55;
    cursor: default;
  }
  .planbtn svg {
    width: 12px;
    height: 12px;
  }
  .part {
    font-size: 12px;
    line-height: 1.5;
  }
  .partrange {
    font-size: 10.5px;
    font-weight: 700;
    color: var(--yap-muted-55);
    font-variant-numeric: tabular-nums;
  }
  .part ul {
    margin: 2px 0 4px;
    padding-left: 18px;
  }
  .part li.ptask {
    list-style: none;
    margin-left: -14px;
  }

  /* "Writing your action plan…" */
  .runline {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 0 10px;
    font-size: 12px;
    color: var(--yap-muted);
  }

  /* The summary step needs an AI model: say so, kindly. */
  .aicard {
    display: flex;
    align-items: center;
    gap: 14px;
    margin: 0 0 12px;
    padding: 12px 14px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg);
    background: var(--yap-primary-wash);
  }
  .aitext {
    flex: 1 1 auto;
    font-size: 12.5px;
    line-height: 1.55;
    color: var(--yap-fg);
  }
  .aidetail {
    display: block;
    margin-top: 4px;
    font-size: 11px;
    color: var(--yap-muted);
  }
  .aibtn {
    flex: 0 0 auto;
    height: 30px;
    padding: 0 13px;
    border: none;
    border-radius: var(--yap-r);
    background: var(--yap-ink, var(--yap-primary));
    color: var(--yap-ink-fg, var(--yap-primary-fg));
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .aibtn:hover {
    background: var(--yap-ink-hover, var(--yap-primary-hover));
  }
  .tcap {
    font-size: 10.5px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--yap-muted-55);
  }
  .tlive {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 11px;
    color: var(--yap-danger);
  }
  .thint {
    margin: 0;
    font-size: 11.5px;
    color: var(--yap-muted);
    line-height: 1.5;
  }
  .tscroll {
    flex: 1 1 auto;
    overflow-y: auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .bubble {
    position: relative;
    max-width: 82%;
    border-radius: 10px;
    padding: 6px 10px;
    font-size: 12px;
    line-height: 1.5;
  }
  .bubble:focus-visible {
    outline: 1.5px solid var(--yap-primary-line);
    outline-offset: 1px;
  }
  .bubble.menuopen {
    box-shadow: 0 0 0 2px var(--yap-border-hover);
  }
  /* The line itself is reading text (Settings → General → Appearance); its
     speaker label and menu keep Yap's font. */
  .bubble p {
    margin: 0;
    overflow-wrap: anywhere;
    font-family: var(--yap-font-reading);
    font-size: calc(12px * var(--yap-reading-scale));
    line-height: calc(1.5 * var(--yap-reading-leading));
  }
  .bubble .who {
    display: block;
    font-size: 9.5px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.07em;
    margin-bottom: 1px;
    color: var(--yap-muted-55);
  }
  /* The next lines of one speaker's run sit close, without the name. */
  .bubble.cont {
    margin-top: -3px;
  }
  .spkwrap {
    position: relative;
    display: inline-block;
  }
  .bubble button.who {
    padding: 0 3px;
    margin: 0 -3px 1px;
    border: none;
    border-radius: 4px;
    background: none;
    font-family: inherit;
    line-height: inherit;
    cursor: pointer;
  }
  .bubble button.who:hover,
  .bubble button.who[aria-expanded='true'] {
    background: var(--yap-s2);
  }
  .bubble button.who.guess {
    font-style: italic;
  }
  .bubble.you {
    align-self: flex-end;
    background: var(--yap-primary-wash);
  }
  .bubble.you .who {
    color: var(--yap-primary);
  }
  .bubble.them {
    align-self: flex-start;
    background: var(--yap-s1);
    border: 1px solid var(--yap-border-subtle);
  }
  .bubble.echo {
    opacity: 0.55;
    border: 1px dashed var(--yap-border);
  }
  /* ▶ a line (meeting_audio.rs): beside it, on hover or focus. */
  .lineplay {
    position: absolute;
    top: 3px;
    right: -25px;
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    padding: 0;
    border: 1px solid var(--yap-border-subtle);
    border-radius: 50%;
    background: var(--yap-s3);
    color: var(--yap-fg-62);
    cursor: pointer;
    opacity: 0;
    transition: opacity 0.12s;
  }
  .bubble.you .lineplay {
    right: auto;
    left: -25px;
  }
  .lineplay svg {
    width: 10px;
    height: 10px;
  }
  .bubble:hover .lineplay,
  .bubble:focus-within .lineplay,
  .lineplay.on {
    opacity: 1;
  }
  .lineplay:hover,
  .lineplay.on {
    color: var(--yap-primary);
  }
  /* What a crash left of its audio (a job's line is MeetingJob.svelte). */
  .trecover {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    border-radius: 8px;
    background: var(--yap-s2);
    font-size: 11.5px;
    line-height: 1.45;
    color: var(--yap-fg-62);
  }
  .trecover span {
    flex: 1;
  }
  /* "You dictated here": a dictation made mid-meeting, left out of "You". */
  .dictated {
    align-self: stretch;
    display: flex;
    align-items: center;
    gap: 7px;
    margin: 1px 0;
    font-size: 10.5px;
    color: var(--yap-muted);
    white-space: nowrap;
  }
  .dictated::before,
  .dictated::after {
    content: '';
    flex: 1 1 auto;
    border-top: 1px dashed var(--yap-border);
  }
  .dictated svg {
    flex: 0 0 auto;
    width: 11px;
    height: 11px;
  }
  .echonote {
    align-self: flex-start;
    border: none;
    background: none;
    padding: 0;
    color: var(--yap-muted);
    font: inherit;
    font-size: 11px;
    text-align: left;
    cursor: pointer;
  }
  .echonote:hover {
    color: var(--yap-fg);
    text-decoration: underline;
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
    background: var(--yap-ink, var(--yap-primary));
    color: var(--yap-ink-fg, var(--yap-primary-fg));
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .enhance.runhalf {
    border-radius: var(--yap-r) 0 0 var(--yap-r);
  }
  .enhance.endmeeting {
    border-radius: var(--yap-r);
    background: var(--yap-danger-fill);
    color: #fff;
    padding: 0 14px;
  }
  .enhance.endmeeting:hover {
    background: var(--yap-danger-fill-hover);
  }
  .enhance.endmeeting svg {
    width: 10px;
    height: 10px;
  }
  .enhance.chevron {
    border-radius: 0 var(--yap-r) var(--yap-r) 0;
    border-left: 1px solid color-mix(in srgb, var(--yap-ink-fg) 22%, transparent);
    padding: 0 7px;
  }
  .enhance:hover:not(:disabled) {
    background: var(--yap-ink-hover, var(--yap-primary-hover));
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
  /* embedded note chat */
  .chatthread {
    flex: 0 0 auto;
    max-height: 220px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: 10px;
    padding: 2px;
  }
  .cbubble {
    max-width: 85%;
    border-radius: 10px;
    padding: 7px 11px;
    font-family: var(--yap-font-reading);
    font-size: calc(12.5px * var(--yap-reading-scale));
    line-height: calc(1.55 * var(--yap-reading-leading));
  }
  .cbubble.user {
    align-self: flex-end;
    background: var(--yap-primary-wash);
  }
  .cbubble.assistant {
    align-self: flex-start;
    background: var(--yap-s2);
    border: 1px solid var(--yap-border-subtle);
  }
  .cbody {
    margin: 0;
    overflow-wrap: anywhere;
  }
  .cbody.thinking {
    color: var(--yap-muted-55);
    font-style: italic;
  }
  .cbubble.assistant :global(p) {
    margin: 4px 0;
  }
  .cbubble.assistant :global(ul),
  .cbubble.assistant :global(ol) {
    margin: 4px 0;
    padding-left: 18px;
  }
  .cbody.plain {
    white-space: pre-wrap;
  }
  /* the model that answered */
  /* Under an answer: ▶ Read aloud (tts.rs), then who answered. */
  .turnfoot {
    align-self: flex-start;
    display: flex;
    align-items: center;
    gap: 6px;
    min-height: 20px;
    margin: -3px 0 2px 2px;
  }
  .readbtn {
    display: inline-flex;
    width: 22px;
    height: 22px;
    align-items: center;
    justify-content: center;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-full);
    background: var(--yap-s1);
    color: var(--yap-muted);
    cursor: pointer;
  }
  .readbtn:hover {
    color: var(--yap-fg);
    border-color: var(--yap-border-hover);
  }
  .readbtn.on {
    color: var(--yap-primary);
    border-color: color-mix(in srgb, var(--yap-primary) 45%, var(--yap-border-subtle));
    background: var(--yap-primary-wash);
  }
  .readbtn svg {
    width: 10px;
    height: 10px;
  }
  .speaking {
    font-size: 10.5px;
    font-weight: 600;
    color: var(--yap-primary);
  }
  .byline {
    font-size: 10.5px;
    color: var(--yap-muted-55);
  }
  .ccopy {
    margin-top: 4px;
    padding: 2px 8px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    background: var(--yap-s1);
    color: var(--yap-muted);
    font: inherit;
    font-size: 11.5px;
    cursor: pointer;
  }
  .ccopy:hover {
    color: var(--yap-fg);
    border-color: var(--yap-border-hover);
  }
  /* a meeting note's one-tap helpers, over the bar */
  .helperrow {
    flex: 0 0 auto;
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-top: 10px;
  }
  .helperchip {
    height: 26px;
    padding: 0 11px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: 999px;
    background: var(--yap-s1);
    color: var(--yap-fg);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .helperchip:hover:not(:disabled) {
    border-color: var(--yap-border-hover);
  }
  .helperchip:disabled {
    opacity: 0.5;
    cursor: default;
  }

  /* the bottom "Ask anything…" bar */
  .askbar {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 10px;
    padding: 8px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
  }
  .micbtn {
    display: inline-flex;
    width: 30px;
    height: 30px;
    align-items: center;
    justify-content: center;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    background: var(--yap-s1);
    color: var(--yap-muted);
    cursor: pointer;
    flex: 0 0 auto;
  }
  .micbtn:hover {
    color: var(--yap-fg);
    border-color: var(--yap-border-hover);
  }
  .micbtn svg {
    width: 13px;
    height: 13px;
  }
  .askinput {
    flex: 1 1 auto;
    min-width: 0;
    height: 30px;
    padding: 0 10px;
    border: none;
    background: transparent;
    color: var(--yap-fg);
    font: inherit;
    font-size: 12.5px;
  }
  .askinput:focus {
    outline: none;
  }
  .askinput::placeholder {
    color: var(--yap-muted-55);
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
    box-shadow: var(--yap-shadow-menu);
  }
  .menu.up {
    top: auto;
    bottom: calc(100% + 6px);
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
    background: var(--yap-key-bg);
    flex: 0 0 auto;
  }

  /* Document-style editor (OpenWhispr NoteEditor: borderless page, not a
     boxed textarea). */
  .raw {
    flex: 1 1 auto;
    min-height: 0;
    resize: none;
    border: none;
    background: transparent;
    color: var(--yap-fg);
    font: inherit;
    /* Reading text: Settings → General → Appearance (app.css). */
    font-family: var(--yap-font-reading);
    font-size: calc(13px * var(--yap-reading-scale));
    line-height: calc(1.7 * var(--yap-reading-leading));
    padding: 4px 2px;
  }
  .raw:focus {
    outline: none;
  }
  .raw::placeholder {
    color: var(--yap-muted-55);
  }

  .rendered {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    border: none;
    background: transparent;
    padding: 4px 2px;
    font-family: var(--yap-font-reading);
    font-size: calc(13px * var(--yap-reading-scale));
    line-height: calc(1.7 * var(--yap-reading-leading));
  }
  .rendered :global(h2),
  .rendered :global(h3),
  .rendered :global(h4),
  .rendered :global(h5) {
    margin: 14px 0 6px;
    line-height: 1.3;
  }
  .rendered :global(h2) {
    font-size: calc(15px * var(--yap-reading-scale));
  }
  .rendered :global(h3) {
    font-size: calc(13.5px * var(--yap-reading-scale));
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
  /* A list under a task: indented under its text, past the checkbox. */
  .rendered :global(li.task > :is(ul, ol)) {
    margin-left: 18px;
  }
  .rendered :global(code) {
    font-family: ui-monospace, Consolas, monospace;
    font-size: calc(12px * var(--yap-reading-step));
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
    background: var(--yap-ink, var(--yap-primary));
    color: var(--yap-ink-fg, var(--yap-primary-fg));
    font: inherit;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .primary:hover {
    background: var(--yap-ink-hover, var(--yap-primary-hover));
  }

  .errline {
    margin: 0 0 10px;
    font-size: 12px;
    color: var(--yap-danger);
    line-height: 1.5;
  }
</style>
