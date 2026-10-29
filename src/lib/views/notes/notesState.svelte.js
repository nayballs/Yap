// The Notes view's state and what it does (NotesView.svelte makes one each
// time Notes opens; its parts — the list, the editor, its chips, summary,
// transcript and chat — read and change it through the object returned):
// the notes and folders, the note open (saved as it's typed, kept in step
// with the meeting notepad and the local API), the meeting recorder, the
// actions that write a summary, the summary edited in place, the transcript
// view, and the note's embedded chat.
//
// Meeting notes: Record / Pause / Resume in the chip row, "End meeting &
// summarise" in the bottom bar; the built-in Action Plan is then written by
// Rust (meetings/meeting_end.rs), whichever window is open, and shown here as it goes
// (docs/meetings.md). The meeting notepad (notepad/Notepad.svelte) shows the
// same note: edits sync both ways.
import { invoke } from '@tauri-apps/api/core';
import { tick } from 'svelte';
import { markdownToText } from '../../core/markdown.js';
import { noteMarkdown, withLineSpeakers, withLineSplice, hasLine, withLineEdit, newSinceSummary } from '../../core/notepadText.js';
import { saveSummary, askReplaceSummary, SUMMARY_KEPT, offerSummaryWords } from '../../core/noteEdits.js';
import { copySummaryForEmail } from '../../core/summaryTools.js';
import { loadNoteChat, openNoteChat, keepTurn, keptMessage, threadMessages } from '../../core/noteChat.js';
import { toast } from '../../ui/toast.svelte.js';
import { showSaveError } from '../../notices/saveErrors.js';
import { summaries, loadSummary, summarise } from '../../stores/meetingSummary.svelte.js';
import { refreshAudio, stopLine } from '../../stores/meetingAudio.svelte.js';
import { speech, initSpeech, readAloud, stopReading } from '../../stores/speech.svelte.js';
import { displayTitle } from './noteFormat.js';
import { markWords, selectWords } from '../../core/findWords.js';

/** This window's label: the `origin` of its saves and stops. */
export const LABEL = 'settings';

/** Errors that mean "no AI model to summarise with" rather than a failure. */
export function needsAiSetup(msg) {
  return /No AI model configured|API key/i.test(msg);
}

/** One Notes view's state. Call it while the view is made (its effects belong to the view). */
export function createNotesState() {
  let notes = $state([]); // list summaries (all folders)
  let folders = $state(['Personal', 'Meetings']);
  let activeFolder = $state('Personal');
  let selected = $state(null); // full note object
  let tab = $state('raw'); // raw | enhanced
  let enhancing = $state(new Set()); // note ids with an enhancement in flight
  let error = $state(null);
  let saveTimer = null;
  // The new folder's name, typed in the list's + or the folder chip's menu.
  let newFolderName = $state('');
  // Actions (named prompt fragments) — the picker + manager dialog.
  let actions = $state([]);
  let actionsOpen = $state(false); // the manager dialog
  let pickerOpen = $state(false); // the split-button dropdown (ActionPicker.svelte)
  let lastUsedActionId = $state(Number(localStorage.getItem('yapLastUsedActionId')) || null);
  const activeAction = $derived(actions.find((a) => a.id === lastUsedActionId) ?? actions[0] ?? null);
  // The built-in "End meeting & summarise" runs (found by kind: users can
  // rename it). Older stores fall back to Meeting Notes.
  const actionPlanAction = $derived(
    actions.find((a) => a.kind === 'actionPlan') ?? actions.find((a) => a.name === 'Meeting Notes') ?? null
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
  // the rolling AI digests (meetings/meeting_summary.rs) via yap-meeting-digest.
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
    aiMissing?.noteId === selected?.id ? aiMissing.message : job?.state === 'needsAi' ? job.error : null
  );

  // ---- the meeting transcript's view ----
  // The transcript box shows the live transcript or the AI notes so far.
  let transcriptView = $state('live');
  // While recording, the live transcript follows the newest lines — unless
  // the user scrolled up to read.
  let logEl = $state(null);
  let followLog = true;
  // "You" chunks that were the call coming through the speakers (meetings/meeting.rs
  // echo check): kept, hidden unless asked for.
  let showEcho = $state(false);
  const shownTranscript = $derived((selected?.transcript || []).filter((s) => showEcho || !s.echo));
  $effect(() => {
    void shownTranscript.length;
    if (logEl && followLog && recordingThisNote) logEl.scrollTop = logEl.scrollHeight;
  });
  // The menu on a speaker's name: `{ key, voice, lines, start }`.
  let speakerMenu = $state(null);
  // The line whose words are being fixed: `{ i, text }` (notes/note_edits.rs).
  let editingLine = $state(null);
  // The transcript's lines, by index (focus, menus, a line opened from Tasks).
  let lineEls = $state({});
  // The line a note was opened at (the Tasks view's "said at 12:40"): lit for a moment.
  let hilite = $state(null);
  // My thoughts' box and the summary as shown: a search's words found there.
  let rawEl = $state(null);
  let renderedEl = $state(null);

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

  function startElapsed(from = 0) {
    elapsed = from;
    clearInterval(elapsedTimer);
    elapsedTimer = setInterval(() => (elapsed += 1), 1000);
  }
  function stopElapsed() {
    clearInterval(elapsedTimer);
    elapsedTimer = null;
  }

  async function startMeeting() {
    if (!selected || meeting.recording) return;
    flushSave();
    error = null;
    if (aiMissing?.noteId === selected.id) aiMissing = null;
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

  // Write the action plan (again): the "Action plan" button, the Action
  // Plan action on a meeting note, and "New since this summary · Update".
  // Rust runs it (meetings/meeting_end.rs), so the meeting notepad shows its
  // progress too. A summary the person edited goes only when they say so.
  async function summariseMeeting(returnFocus = null) {
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
    finishSummaryEdit();
    if (selected.summaryEditedMs && selected.enhancedContent?.trim()) {
      askReplaceSummary({ run: () => startMeetingSummary(true), returnFocus });
      return;
    }
    await startMeetingSummary(false);
  }
  async function startMeetingSummary(replace) {
    if (!selected) return false;
    error = null;
    if (aiMissing?.noteId === selected.id) aiMissing = null;
    try {
      await summarise(selected.id, { replace });
    } catch (e) {
      toast({ title: "Couldn't write the action plan", description: String(e), variant: 'destructive' });
      return false;
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
  // back, and the next keystroke here would save it). The notepad does the
  // same.
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
      if (summaryDirty) n.enhancedContent = selected.enhancedContent;
      for (const f of Object.keys(changed)) if (changed[f] !== before[f]) n[f] = selected[f];
      selected = n;
    } catch {
      /* keep current */
    }
  }

  // The action plan Rust writes (meetings/meeting_end.rs): bring it in when done.
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
    if (typeof p.enhancedContent === 'string' && !summaryDirty) {
      changing('enhancedContent', 'summaryEditedMs', 'summaryTs', 'summaryAi', 'summaryRating');
      selected.enhancedContent = p.enhancedContent;
      if (typeof p.summaryEditedMs === 'number') selected.summaryEditedMs = p.summaryEditedMs;
      if (typeof p.summaryTs === 'number') selected.summaryTs = p.summaryTs;
      if ('summaryAi' in p) selected.summaryAi = p.summaryAi || '';
      if ('summaryRating' in p) selected.summaryRating = p.summaryRating ?? null;
    }
    // A line's words fixed in the meeting notepad.
    if (p.lineEdit) {
      const t = withLineEdit(selected.transcript, p.lineEdit);
      if (t) {
        changing('transcript');
        selected.transcript = t;
      } else reloadSelected();
    }
    // The voice separation labelled lines (`lineSplice`, some split where
    // the speaker changed), or a speaker was renamed or merged in the
    // notepad (`lineSpeakers`): every line's voice.
    if (p.lineSplice || p.lineSpeakers) {
      const t = p.lineSplice ? withLineSplice(selected.transcript, p) : withLineSpeakers(selected.transcript, p.lineSpeakers);
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

  // ---- the notes and their folders ----

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

  async function select(id) {
    flushSave();
    finishSummaryEdit();
    editingLine = null;
    try {
      selected = await invoke('note_get', { id });
      loadSummary(id);
      tab = 'raw';
      transcriptView = 'live';
      showEcho = false;
      error = null;
      // The embedded chat is pinned to one note: its kept thread, when it
      // has one (chat/note_chats.rs).
      chatThread = [];
      chatConvId = null;
      chatThreads = [];
      chatInput = '';
      stopReading('notes:');
      loadChat(id);
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
      if (!showSaveError(e)) {
        error = String(e);
        return;
      }
      // Made but not saved yet (Yap keeps it): open it, the newest note.
      await refreshList();
      const newest = notes.reduce((a, n) => (!a || n.id > a.id ? n : a), null);
      if (newest) await select(newest.id);
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
      // Deleted but not saved yet: gone here, and Yap says so.
      if (!showSaveError(e)) {
        toast({ title: "Couldn't delete the note", description: String(e), variant: 'destructive' });
        refreshList();
        return false;
      }
    }
    if (selected?.id === id) selected = null;
    refreshList();
    return true;
  }

  // The open note's audio kept on disk (meetings/meeting_audio.rs): ▶ on its lines,
  // what a crash left of it; kept current from `yap-meeting-audio`.
  let audioFor = null;
  $effect(() => {
    const id = selected?.id ?? null;
    if (id === audioFor) return;
    audioFor = id;
    stopLine();
    if (id != null) refreshAudio(id);
  });

  // The title box (Rename in a row's menu focuses it).
  let titleEl = $state(null);
  async function renameNote(id) {
    if (selected?.id !== id) await select(id);
    if (selected?.id !== id) return;
    setTimeout(() => {
      titleEl?.focus();
      titleEl?.select();
    }, 0);
  }
  // Read a note aloud (voice/read_aloud.rs: the reading card on the Yap bar, its
  // words lit up as they're said; it reads on if you leave Notes): the open
  // note as it's shown — the Enhanced summary on that tab, else the note —
  // or note `id` (its row's menu). Again while it reads: Stop.
  const readingNote = (id) => speech.speakingId === `note:${id}`;
  async function readNote(id = selected?.id) {
    if (id == null) return;
    if (selected?.id === id) flushSave();
    try {
      const n = selected?.id === id ? selected : await invoke('note_get', { id });
      const body =
        selected?.id === id && tab === 'enhanced' && n.enhancedContent ? n.enhancedContent : n.content || n.enhancedContent || '';
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
    // A failed save keeps the edit in memory and says so, with Try again.
    invoke('note_update', args)
      .then(refreshList)
      .catch((e) => showSaveError(e));
  }

  const isStale = $derived.by(() => {
    if (!selected || !selected.enhancedContent) return false;
    const s = notes.find((n) => n.id === selected.id);
    return !!s?.stale;
  });

  // ---- the summary, edited in place (notes/note_edits.rs) ----
  let summaryEditing = $state(false);
  let summaryDraft = $state('');
  let summaryEl = $state(null);
  let summaryTimer = null;
  // Typed and not yet saved: an event or a read never puts older words back.
  let summaryDirty = false;
  // "3 new lines since this summary · Update" (not while it records: the
  // summary is written when it stops).
  const newLines = $derived(recordingThisNote ? 0 : newSinceSummary(selected));
  // The summary as it was at Edit: a word swapped by Done is offered for
  // the dictionary (noteEdits.js `offerSummaryWords`).
  let summaryBefore = '';
  async function startSummaryEdit() {
    if (!selected) return;
    summaryDraft = selected.enhancedContent || '';
    summaryBefore = summaryDraft;
    summaryEditing = true;
    await tick();
    summaryEl?.focus();
    fitSummary();
  }
  function fitSummary() {
    if (!summaryEl) return;
    summaryEl.style.height = 'auto';
    summaryEl.style.height = `${summaryEl.scrollHeight + 2}px`;
  }
  function queueSummarySave() {
    summaryDirty = true;
    fitSummary();
    clearTimeout(summaryTimer);
    summaryTimer = setTimeout(flushSummary, 600);
  }
  async function flushSummary() {
    clearTimeout(summaryTimer);
    summaryTimer = null;
    if (!summaryDirty || !selected) return;
    summaryDirty = false;
    const id = selected.id;
    const text = summaryDraft;
    try {
      const s = await saveSummary(id, text, LABEL);
      if (selected?.id !== id) return;
      changing('enhancedContent', 'summaryEditedMs', 'summaryTs', 'summaryAi', 'summaryRating');
      if (!summaryDirty) selected.enhancedContent = s.enhancedContent;
      selected.summaryEditedMs = s.summaryEditedMs;
      selected.summaryTs = s.summaryTs;
      if ('summaryAi' in s) selected.summaryAi = s.summaryAi || '';
      if ('summaryRating' in s) selected.summaryRating = s.summaryRating ?? null;
      refreshList();
    } catch (e) {
      toast({ title: "Couldn't save the summary", description: String(e), variant: 'destructive' });
    }
  }
  function finishSummaryEdit() {
    if (!summaryEditing) return;
    if (summaryDirty && selected) selected.enhancedContent = summaryDraft;
    flushSummary();
    summaryEditing = false;
    offerSummaryWords(summaryBefore, summaryDraft);
  }

  // ---- actions: a summary written from the note (note_enhance) ----

  /** Whether an action can run on the open note now. */
  function canRun(action) {
    return !!selected && !enhancing.has(selected.id) && !jobRunning && !!action;
  }
  async function runAction(action, returnFocus = null) {
    if (!canRun(action)) return;
    pickerOpen = false;
    lastUsedActionId = action.id;
    localStorage.setItem('yapLastUsedActionId', String(action.id));
    // A meeting's action plan is Rust's job (progress shows in the notepad
    // too); any other action runs here.
    if (action.kind === 'actionPlan' && selected.transcript?.length) {
      summariseMeeting(returnFocus);
      return;
    }
    flushSave();
    finishSummaryEdit();
    // A summary the person edited goes only when they say so (notes/note_edits.rs).
    if (selected.summaryEditedMs && selected.enhancedContent?.trim()) {
      askReplaceSummary({ run: () => runActionNow(action, true), returnFocus });
      return;
    }
    await runActionNow(action, false);
  }
  async function runActionNow(action, replace) {
    if (!selected) return false;
    const id = selected.id;
    enhancing = new Set([...enhancing, id]);
    runningAction = { noteId: id, name: action.name };
    error = null;
    if (aiMissing?.noteId === id) aiMissing = null;
    try {
      const enhanced = await invoke('note_enhance', { id, actionId: action.id, replace });
      // Only mutate the open note if the user is still on it.
      if (selected?.id === id) {
        selected.enhancedContent = enhanced;
        tab = 'enhanced'; // OpenWhispr auto-switches to the Enhanced tab
        try {
          const n = await invoke('note_get', { id });
          selected.digests = n.digests;
          selected.summaryEditedMs = n.summaryEditedMs;
          selected.summaryTs = n.summaryTs;
        } catch {
          /* keep current */
        }
      }
      // The Enhanced tab, opened on the result, says it (no toast on top).
      refreshList();
    } catch (e) {
      const msg = String(e);
      if (msg === SUMMARY_KEPT) {
        // Edited again while it ran: the person's words stay.
        toast({ kind: 'done', title: 'Your summary was kept', description: 'You changed it while Yap was writing a new one.' });
      } else if (showSaveError(msg)) {
        // Written, but not on disk yet (said with Try again): show it.
        if (selected?.id === id) {
          try {
            const n = await invoke('note_get', { id });
            selected.enhancedContent = n.enhancedContent;
            selected.summaryEditedMs = n.summaryEditedMs;
            selected.summaryTs = n.summaryTs;
            tab = 'enhanced';
          } catch {
            /* keep current */
          }
        }
      } else if (needsAiSetup(msg)) {
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

  // The summary for an email: rich (headings, lists, tasks) and plain text
  // on the clipboard at once (core/summaryTools.js).
  async function copyForEmail() {
    if (!selected?.enhancedContent?.trim()) return;
    if (await copySummaryForEmail(selected)) {
      copied = 'email';
      setTimeout(() => (copied = false), 1500);
      toast({ title: 'Summary copied for email', description: 'Paste it into your email: headings, lists and tasks come with it.', variant: 'success' });
    } else {
      toast({ title: "Couldn't copy", description: 'The clipboard is busy. Try again.', variant: 'destructive' });
    }
  }

  // Copy the open tab: markdown as is, or as plain text (headings and
  // checkboxes spelled out) for chat apps and email.
  let copied = $state(false);
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

  // The folder chip's menu (NoteMeta.svelte): a move closes it, wherever
  // it was asked for.
  let folderMenuOpen = $state(false);
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
      toast({ title: `Moved to ${f}`, variant: 'success' });
    } catch (e) {
      if (showSaveError(e)) refreshList();
      else toast({ title: "Couldn't move note", description: String(e), variant: 'destructive' });
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

  // ---- the note's embedded chat (the "Ask anything…" bar) ----
  // { role: 'user'|'assistant', text, model? } — `model`: who answered
  // (typed questions go to the Chat scope, the helpers to Note Formatting).
  let chatThread = $state([]);
  let chatInput = $state('');
  let asking = $state(false);
  // Each answer's id for ▶ Read aloud (speech.svelte.js / voice/tts.rs), and a
  // note's own ("note:<id>").
  initSpeech(['notes:', 'note:']);
  let turnSeq = 0;
  const turnId = () => `notes:${selected?.id ?? 0}:${++turnSeq}`;

  // The thread kept with the note (chat/note_chats.rs, core/noteChat.js):
  // shown when the note opens, each turn kept as it's shown, the notepad's
  // turns followed (`yap-note-chat`); New chat, and Past chats.
  let chatConvId = $state(null);
  let chatThreads = $state([]);
  /** Note `id`'s newest thread, or thread `conversation` of it. */
  async function loadChat(id, conversation = null) {
    try {
      const chat = await loadNoteChat(id);
      const c = conversation != null ? await openNoteChat(id, conversation) : chat.current;
      if (selected?.id !== id || asking) return;
      chatThreads = chat.threads || [];
      chatConvId = c?.id ?? null;
      chatThread = threadMessages(c, `notes:${id}:`).map((m) => ({ ...m, copy: m.plain || undefined }));
    } catch {
      /* none kept */
    }
  }
  /** Keep a turn as shown in the note's thread. */
  async function keepChat(id, messages) {
    try {
      const conv = await keepTurn(id, chatConvId, messages.map(keptMessage), LABEL);
      if (selected?.id !== id) return;
      chatConvId = conv;
      chatThreads = (await loadNoteChat(id)).threads || chatThreads;
    } catch (e) {
      showSaveError(e);
    }
  }
  /** New chat: the next question starts another thread (this one stays). */
  function newChat() {
    chatThread = [];
    chatConvId = null;
    chatInput = '';
  }
  /** The meeting notepad added to this note's thread. */
  function onNoteChat(p) {
    if (!p || p.origin === LABEL || selected?.id !== p.noteId || asking) return;
    if (chatConvId === p.conversationId || (chatConvId == null && !chatThread.length)) loadChat(p.noteId, p.conversationId);
    else loadNoteChat(p.noteId).then((c) => selected?.id === p.noteId && (chatThreads = c.threads || [])).catch(() => {});
  }

  // ---- a note opened from elsewhere ----

  // The transcript line a note was opened at (the Tasks view's "said at
  // 12:40"): scrolled to, focused and lit for a moment.
  async function showLine(i) {
    transcriptView = 'live';
    showEcho = showEcho || !!selected?.transcript?.[i]?.echo;
    await tick();
    const el = lineEls[i];
    if (!el) return;
    followLog = false;
    el.scrollIntoView({ block: 'center' });
    el.focus({ preventScroll: true });
    hilite = i;
    setTimeout(() => hilite === i && (hilite = null), 2400);
  }

  /**
   * Open note `id` (at transcript line `line`; `ts`: when that line was
   * said, to find it if lines moved since; `tab`: 'enhanced' | 'raw'), and
   * end its meeting if `stop`. `find`: a search's `{ words, line }` — found
   * again there (core/findWords.js): marked in the transcript line or the
   * summary, selected in My thoughts at that line.
   */
  async function openRequested({ id, stop, line, ts = null, tab: onTab = null, find = null }) {
    await select(id);
    if (selected?.id !== id) return;
    activeFolder = selected.folder || activeFolder;
    refreshList();
    if (onTab === 'enhanced' && selected.enhancedContent) tab = 'enhanced';
    else if (onTab === 'raw') tab = 'raw';
    if (line != null) {
      const lines = selected.transcript || [];
      let at = line;
      if (ts != null && lines[at]?.ts !== ts) {
        // Lines moved since (a transcript done again): the one said then.
        const found = lines.findIndex((s) => s.ts === ts);
        if (found >= 0) at = found;
      }
      at = Math.min(at, Math.max(lines.length - 1, 0));
      await showLine(at);
      if (find?.words?.length) markWords(lineEls[at], find.words, { scroll: false });
    } else if (find?.words?.length) {
      await tick();
      if (tab === 'enhanced') markWords(renderedEl, find.words);
      else selectWords(rawEl, find.words, find.line ?? null);
    }
    if (!stop) return;
    meeting = (await invoke('meeting_state').catch(() => null)) || { recording: false };
    if (meeting.recording && meeting.noteId === id) await endMeeting();
  }

  return {
    get notes() { return notes; },
    get folders() { return folders; },
    set folders(v) { folders = v; },
    get activeFolder() { return activeFolder; },
    set activeFolder(v) { activeFolder = v; },
    get selected() { return selected; },
    set selected(v) { selected = v; },
    get tab() { return tab; },
    set tab(v) { tab = v; },
    get enhancing() { return enhancing; },
    get error() { return error; },
    set error(v) { error = v; },
    get copied() { return copied; },
    get newFolderName() { return newFolderName; },
    set newFolderName(v) { newFolderName = v; },
    get actions() { return actions; },
    get actionsOpen() { return actionsOpen; },
    set actionsOpen(v) { actionsOpen = v; },
    get pickerOpen() { return pickerOpen; },
    set pickerOpen(v) { pickerOpen = v; },
    get activeAction() { return activeAction; },
    get actionPlanAction() { return actionPlanAction; },
    get meeting() { return meeting; },
    set meeting(v) { meeting = v; },
    get elapsed() { return elapsed; },
    get recordingThisNote() { return recordingThisNote; },
    get job() { return job; },
    get jobRunning() { return jobRunning; },
    get summaryProgress() { return summaryProgress; },
    get needsAi() { return needsAi; },
    get transcriptView() { return transcriptView; },
    set transcriptView(v) { transcriptView = v; },
    get logEl() { return logEl; },
    set logEl(v) { logEl = v; },
    get followLog() { return followLog; },
    set followLog(v) { followLog = v; },
    get showEcho() { return showEcho; },
    set showEcho(v) { showEcho = v; },
    get speakerMenu() { return speakerMenu; },
    set speakerMenu(v) { speakerMenu = v; },
    get editingLine() { return editingLine; },
    set editingLine(v) { editingLine = v; },
    get lineEls() { return lineEls; },
    get hilite() { return hilite; },
    get rawEl() { return rawEl; },
    set rawEl(v) { rawEl = v; },
    get renderedEl() { return renderedEl; },
    set renderedEl(v) { renderedEl = v; },
    get titleEl() { return titleEl; },
    set titleEl(v) { titleEl = v; },
    get isStale() { return isStale; },
    get summaryEditing() { return summaryEditing; },
    get summaryDraft() { return summaryDraft; },
    set summaryDraft(v) { summaryDraft = v; },
    get summaryEl() { return summaryEl; },
    set summaryEl(v) { summaryEl = v; },
    get newLines() { return newLines; },
    get runningAction() { return runningAction; },
    get folderMenuOpen() { return folderMenuOpen; },
    set folderMenuOpen(v) { folderMenuOpen = v; },
    get chatThread() { return chatThread; },
    set chatThread(v) { chatThread = v; },
    get chatInput() { return chatInput; },
    set chatInput(v) { chatInput = v; },
    get asking() { return asking; },
    set asking(v) { asking = v; },
    get chatConvId() { return chatConvId; },
    get chatThreads() { return chatThreads; },
    loadChat,
    keepChat,
    newChat,
    onNoteChat,
    refreshActions,
    startElapsed,
    stopElapsed,
    startMeeting,
    pauseMeeting,
    endMeeting,
    summariseMeeting,
    onMeetingState,
    changing,
    reloadSelected,
    onSummary,
    onNoteChanged,
    onSpeakerChanged,
    openNotepad,
    onMeetingSegment,
    onMeetingDigest,
    refreshList,
    select,
    newNote,
    removeNote,
    renameNote,
    readingNote,
    readNote,
    copyNoteMarkdown,
    queueSave,
    flushSave,
    startSummaryEdit,
    queueSummarySave,
    finishSummaryEdit,
    canRun,
    runAction,
    copyEnhanced,
    copyForEmail,
    moveToFolder,
    onNoteFiled,
    exportNote,
    turnId,
    openRequested,
    showLine,
  };
}
