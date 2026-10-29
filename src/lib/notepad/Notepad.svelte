<script>
  // The meeting notepad (meetings/notepad.rs) — Yap's meeting UI while a call runs,
  // matched to Wispr Flow's Notetaker notepad from measurements of its DOM
  // (sizes, type, colours and motion; the palette is `--yap-paper-*` in
  // app.css). This file is the window: the note shown and the meeting, kept
  // in step with Rust's events and the other windows; its parts, top to
  // bottom, are their own components in src/lib/notepad/:
  //   - NotepadHeader: back, ⋯, split screen, Share, the window buttons;
  //   - NotepadTitle and NotepadTabs: the title and date; My thoughts,
  //     Transcript, Summary;
  //   - the tab shown: ThoughtsTab (the note's own text, synced with the
  //     Notes view both ways: `yap-note-changed`, `origin` = window label),
  //     TranscriptTab (+ TranscriptLog) or SummaryTab — a tab's body is made
  //     again on every switch, so what it shows that must last lives here
  //     (TranscriptView, SummaryEditor);
  //   - at the bottom: GenerateSummary (or the summary's error, with Retry),
  //     ConsentLine while recording (ConsentMessage), and AskTray — Stop /
  //     Resume and "Ask anything", the sheet chat about the meeting;
  //   - DeleteMeetingDialog.
  // Like every Yap window you can type in, it catches the dictation key and
  // the meeting shortcut in-page (a focused WebView2 window never reaches the
  // global hook).
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { onMount } from 'svelte';
  import ToastHost from '../ui/ToastHost.svelte';
  import { toast } from '../ui/toast.svelte.js';
  import { menuOrDialogOpen } from '../ui/contextMenu.svelte.js';
  import { showSaveError } from '../notices/saveErrors.js';
  import { initMeetingCheckIns } from '../notices/meetingCheckIn.js';
  import { initSpeech, readPageSelection } from '../stores/speech.svelte.js';
  import { summaries, initMeetingSummary, loadSummary, summarise, askStartedByMistake } from '../stores/meetingSummary.svelte.js';
  import { initMeetingAudio, refreshAudio, stopLine } from '../stores/meetingAudio.svelte.js';
  import { actionMatchesKeydown, actionMatchesKeyup } from '../core/hotkeys.js';
  import { withLineSpeakers, withLineSplice, hasLine, withLineEdit } from '../core/notepadText.js';
  import { askReplaceSummary } from '../core/noteEdits.js';
  import { LABEL } from './actions.js';
  import { TranscriptView } from './transcriptView.svelte.js';
  import { SummaryEditor } from './summaryEditor.svelte.js';
  import { ConsentMessage } from './consentMessage.svelte.js';
  import NotepadHeader from './NotepadHeader.svelte';
  import NotepadTitle from './NotepadTitle.svelte';
  import NotepadTabs from './NotepadTabs.svelte';
  import ThoughtsTab from './ThoughtsTab.svelte';
  import TranscriptTab from './TranscriptTab.svelte';
  import SummaryTab from './SummaryTab.svelte';
  import MeetingTasks from '../components/MeetingTasks.svelte';
  import GenerateSummary from './GenerateSummary.svelte';
  import ConsentLine from './ConsentLine.svelte';
  import AskTray from './AskTray.svelte';
  import CoachMark from './CoachMark.svelte';
  import DeleteMeetingDialog from './DeleteMeetingDialog.svelte';
  import Icon from './NotepadIcon.svelte';
  import './notepad.css';

  const appWindow = getCurrentWindow();

  let note = $state(null); // the meeting note shown (full)
  let meeting = $state({ recording: false });
  let elapsed = $state(0);
  let elapsedTimer = null;
  let stopping = $state(false);
  let tab = $state('thoughts'); // thoughts | transcript | summary | tasks
  let cfg = null; // for the in-page hotkey fallback
  // "Show live transcript" (Settings → Meetings): off, the
  // Transcript tab stays quiet until the meeting stops.
  let liveTranscript = $state(true);
  // The popover open, one at a time: 'more' | 'share' | 'consent' | null.
  let menu = $state(null);
  let confirmOpen = $state(false);
  // The Ask sheet (AskTray: `close`, `reset`).
  let askOpen = $state(false);
  let askTray = $state(null);
  // The window is on screen (for what counts as seen).
  let windowShown = $state(true);

  // What the tabs show that must outlast a tab switch.
  const view = new TranscriptView();
  const summary = new SummaryEditor({ note: () => note, changing });
  const consent = new ConsentMessage((message) => {
    if (cfg) cfg.meetingConsentMessage = message;
  });

  const recordingThis = $derived(!!note && meeting.recording && meeting.noteId === note.id);
  const job = $derived(note ? summaries.byNote[note.id] : null);
  const progress = $derived(note ? summaries.progress[note.id] : null);
  const writing = $derived(job?.state === 'running');
  const hasSummary = $derived(!!note?.enhancedContent?.trim());
  const lineCount = $derived((note?.transcript || []).length);
  // The meeting's audio kept on disk (meetings/meeting_audio.rs): ⋯ → Transcribe
  // again, ▶ on its lines, what a crash left of it.
  let audioFor = null;
  $effect(() => {
    const id = note?.id ?? null;
    if (id === audioFor) return;
    audioFor = id;
    stopLine();
    if (id != null) refreshAudio(id);
  });

  // ---- loading and saving ----

  let loads = 0;
  async function load(id) {
    flushSave();
    summary.finish();
    view.editingLine = null;
    const asked = ++loads;
    if (id == null) {
      note = null;
      return;
    }
    // The note already shown (a recording resumed in it): only what changed,
    // never over what arrived meanwhile.
    if (note?.id === id) {
      loadSummary(id);
      return reload();
    }
    try {
      const n = await invoke('note_get', { id });
      if (asked !== loads) return; // another note was asked for since
      if (note?.id !== n.id) {
        tab = 'thoughts';
        askOpen = false;
        askTray?.reset();
        view.reset();
        menu = null;
        confirmOpen = false;
      }
      note = n;
      loadSummary(n.id);
    } catch {
      if (asked === loads) note = null;
    }
  }

  // What the events below changed, field by field: a reload takes only the
  // fields no event changed while its read was on its way. Without that, a
  // read answered just before a change and delivered just after it put the
  // older value back — a stop's paused divider (`yap-meeting-breaks` lands
  // while the reload the stop started is still out), a line, a summary, the
  // text typed in the Notes view — until something reloaded again.
  const changed = { title: 0, content: 0, participants: 0, transcript: 0, speakers: 0, breaks: 0, digests: 0, summary: 0 };
  function changing(...fields) {
    for (const f of fields) changed[f] += 1;
  }
  let reloads = 0;
  // The note's fields those events carry (the rest only a read brings).
  const EVENT_FIELDS = new Set([
    'title',
    'titleAuto',
    'content',
    'participants',
    'transcript',
    'speakers',
    'breaks',
    'digests',
    'enhancedContent',
    'summaryEditedMs',
    'summaryTs',
    'summaryAi',
    'summaryRating',
  ]);

  async function reload() {
    if (!note) return;
    const asked = ++reloads;
    const before = { ...changed };
    const kept = (f) => changed[f] === before[f];
    try {
      const n = await invoke('note_get', { id: note.id });
      // Another note, or a newer reload on its way: that one says it.
      if (note?.id !== n.id || asked !== reloads) return;
      // Keep what's being typed, and whatever arrived meanwhile; take the rest.
      if (!dirty.title && kept('title')) {
        note.title = n.title;
        note.titleAuto = n.titleAuto;
      }
      if (!dirty.content && kept('content')) note.content = n.content;
      if (kept('transcript')) note.transcript = n.transcript;
      if (kept('speakers')) note.speakers = n.speakers;
      if (kept('breaks')) note.breaks = n.breaks;
      if (kept('digests')) note.digests = n.digests;
      if (kept('summary') && !summary.dirty) {
        note.enhancedContent = n.enhancedContent;
        note.summaryEditedMs = n.summaryEditedMs;
        note.summaryTs = n.summaryTs;
        // Yap's own version and the person's rating (absent: none).
        note.summaryAi = n.summaryAi || '';
        note.summaryRating = n.summaryRating ?? null;
      }
      if (kept('participants')) note.participants = n.participants;
      // The rest as stored: no event brings them (its calendar meeting, tied
      // as the recording started — its brief follows —, the date, the type).
      for (const [k, v] of Object.entries(n)) if (!EVENT_FIELDS.has(k)) note[k] = v;
      if (!('event' in n)) note.event = undefined;
    } catch {
      if (asked === reloads) note = null; // gone (deleted)
    }
  }

  // ---- the meeting's pre-meeting brief (calendar/brief.rs) ----
  // At the top of the Summary tab: unfolded until the meeting has its action
  // plan, then folded under "Brief before the meeting". While the meeting
  // records, a slim line over My thoughts says what's in it and opens it.
  let brief = $state(null);
  // null: as the meeting goes (open until there's an action plan); else the
  // person's own choice.
  let briefFold = $state(null);
  let briefLineGone = $state(false);
  let briefFor = '';
  $effect(() => {
    const id = note?.id ?? null;
    const key = note?.event?.key ?? '';
    if (`${id}:${key}` === briefFor) return;
    if (!briefFor.startsWith(`${id}:`)) {
      briefLineGone = false;
      briefFold = null;
    }
    briefFor = `${id}:${key}`;
    brief = null;
    if (id == null || !key) return;
    invoke('calendar_brief_for_note', { noteId: id })
      .then((b) => {
        if (note?.id === id) brief = b;
      })
      .catch(() => {});
  });
  function onBrief(b) {
    if (b?.key && note?.event?.key === b.key) brief = b;
  }
  function openBrief() {
    tab = 'summary';
    briefFold = false;
  }

  // Debounced autosave of whatever was typed. Only the edited fields are
  // sent, so a title the AI just wrote (or an edit made in the Notes view)
  // is never overwritten by a stale copy here.
  let saveTimer = null;
  let dirty = { title: false, content: false };
  function queueSave(field) {
    dirty[field] = true;
    clearTimeout(saveTimer);
    saveTimer = setTimeout(flushSave, 600);
  }
  function flushSave() {
    if (saveTimer === null) return;
    clearTimeout(saveTimer);
    saveTimer = null;
    if (!note) return;
    const args = { id: note.id, origin: LABEL };
    if (dirty.title) args.title = note.title;
    if (dirty.content) args.content = note.content;
    dirty = { title: false, content: false };
    // A failed save keeps the edit in memory and says so, with Try again.
    invoke('note_update', args).catch((e) => showSaveError(e));
  }

  // Another window (or the AI title) changed the note.
  function onNoteChanged(p) {
    if (!note || p?.id !== note.id || p.origin === LABEL) return;
    // The speaker engine's labels change only the lines and the map: never
    // touch what's being typed for them (they come every few seconds).
    if (p.origin !== 'speakers') {
      changing('title', 'content', 'participants');
      if (!dirty.title) {
        note.title = p.title;
        note.titleAuto = p.titleAuto;
      }
      if (!dirty.content) note.content = p.content;
      note.participants = p.participants;
    }
    if (p.speakers) {
      changing('speakers');
      note.speakers = p.speakers;
    }
    if (typeof p.enhancedContent === 'string' && !summary.dirty) {
      changing('summary');
      note.enhancedContent = p.enhancedContent;
      if (typeof p.summaryEditedMs === 'number') note.summaryEditedMs = p.summaryEditedMs;
      if (typeof p.summaryTs === 'number') note.summaryTs = p.summaryTs;
      if ('summaryAi' in p) note.summaryAi = p.summaryAi || '';
      if ('summaryRating' in p) note.summaryRating = p.summaryRating ?? null;
    }
    if (p.lineEdit) {
      // A line's words fixed in the Notes view.
      const t = withLineEdit(note.transcript, p.lineEdit);
      if (t) {
        changing('transcript');
        note.transcript = t;
      } else reload();
    }
    if (p.lineSplice) {
      // The voice separation labelled lines, splitting some where the
      // speaker changed: the stretch that changed.
      const t = withLineSplice(note.transcript, p);
      if (t) {
        changing('transcript', 'breaks');
        note.transcript = t;
        if (p.breaks) note.breaks = p.breaks;
      } else reload();
    } else if (p.lineSpeakers) {
      // A speaker renamed or merged in another window: every line's voice.
      const t = withLineSpeakers(note.transcript, p.lineSpeakers);
      if (t) {
        changing('transcript');
        note.transcript = t;
      } else reload();
    }
  }

  /** A speaker edit made here: the map, each line's voice (and echo: "It
   *  was me"), and the summary (a name put on "Speaker 2"). */
  function onSpeakerChanged(updated) {
    if (!note || updated?.id !== note.id) return;
    changing('speakers', 'transcript', 'summary');
    note.speakers = updated.speakers || {};
    const t = updated.transcript || [];
    note.transcript = (note.transcript || []).map((s, i) =>
      t[i] ? { ...s, speaker: t[i].speaker || '', speakerLocked: !!t[i].speakerLocked, echo: !!t[i].echo } : s
    );
    if (typeof updated.enhancedContent === 'string') note.enhancedContent = updated.enhancedContent;
  }

  // ---- the meeting ----

  function startElapsed(from = 0) {
    elapsed = from;
    clearInterval(elapsedTimer);
    elapsedTimer = setInterval(() => (elapsed += 1), 1000);
  }
  function stopElapsed() {
    clearInterval(elapsedTimer);
    elapsedTimer = null;
  }

  function onMeetingState(s) {
    const was = meeting.recording;
    const wasNote = meeting.noteId;
    meeting = s || { recording: false };
    if (meeting.recording) {
      startElapsed(meeting.elapsedSecs || 0);
      if (!was) refreshConfig(); // "Show live transcript" may have changed
      if (speakersNote?.noteId === meeting.noteId) speakersNote = null;
    } else {
      stopElapsed();
      stopping = false;
      // The last chunk is in: the transcript as stored.
      if (was && note && wasNote === note.id) reload();
    }
  }

  function onSegment(seg) {
    if (!seg || !recordingThis || hasLine(note.transcript, seg)) return;
    changing('transcript');
    note.transcript = [...(note.transcript || []), seg];
  }

  // The speaker engine stopped early (speakers/labels.rs): said quietly
  // under the transcript, for this note, until it records again.
  let speakersNote = $state(null);
  function onSpeakers(p) {
    if (p?.noteId != null && p.message) speakersNote = { noteId: p.noteId, message: p.message };
  }

  function onDigest(p) {
    if (!p?.digest || note?.id !== p.noteId) return;
    changing('digests');
    note.digests = [...(note.digests || []), p.digest];
  }

  // A recording stopped: where, for the paused divider.
  function onBreaks(p) {
    if (!note || p?.noteId !== note.id) return;
    changing('breaks');
    note.breaks = p.breaks;
  }

  // Stop = "End meeting & summarise": Rust writes the action plan once the
  // last few seconds are transcribed (or asks "Started by mistake?").
  async function stop() {
    if (!recordingThis || stopping) return;
    flushSave();
    stopping = true;
    try {
      await invoke('meeting_end', { origin: LABEL });
    } catch (e) {
      stopping = false;
      toast({ title: "Couldn't stop recording", description: String(e), variant: 'destructive' });
    }
  }

  async function resume() {
    if (!note || meeting.recording) return;
    flushSave();
    try {
      await invoke('meeting_start', { noteId: note.id });
      tab = 'transcript';
    } catch (e) {
      toast({ title: "Couldn't start recording", description: String(e), variant: 'destructive' });
    }
  }

  async function generate(returnFocus = null) {
    if (!note || writing) return;
    flushSave();
    summary.finish();
    // A summary the person edited goes only when they say so (notes/note_edits.rs).
    if (note.summaryEditedMs && hasSummary) {
      askReplaceSummary({ run: () => startSummary(true), returnFocus });
      return;
    }
    await startSummary(false);
  }
  async function startSummary(replace) {
    tab = 'summary';
    try {
      await summarise(note.id, { replace });
    } catch (e) {
      toast({ title: "Couldn't write the summary", description: String(e), variant: 'destructive' });
      return false;
    }
  }

  // Every new run of the summary shows on the Summary tab; a finished one
  // brings the plan in.
  let lastRun = null;
  function onSummary(s) {
    if (!note || s?.noteId !== note.id) return;
    if (s.state === 'running' && s.run !== lastRun) tab = 'summary';
    lastRun = s.run;
    if (s.state === 'done') reload();
  }

  // ---- an answer's cited line (Make me sound smart, a Skill) ----

  /** A passage an answer cites: its line on the Transcript tab (lit a
   *  moment), or the Summary / My thoughts it came from. */
  function openSource(s) {
    if (!note || s?.note !== note.id) return;
    askTray?.close();
    if (s.field === 'summary') tab = 'summary';
    else if (s.field === 'content') tab = 'thoughts';
    else {
      const t = note.transcript || [];
      // Lines moved since (a transcript done again): the one said then.
      const i = t[s.line]?.ts === s.ts || !s.ts ? s.line : t.findIndex((x) => x.ts === s.ts);
      tab = 'transcript';
      if (i != null && i >= 0) view.jumpTo(i);
    }
  }

  // ---- first-meeting coach marks (meetings/coach_marks.rs) ----
  // In the person's first recorded meeting: "Take notes here" over My
  // thoughts and "Try What did I miss?" by the Ask bar, each once (counted
  // as it shows), each dismissable; typing notes or opening the Ask sheet
  // puts the one it's about away.
  let coach = $state({ notes: false, catchUp: false });
  let coachFor = null;
  const counted = new Set();
  $effect(() => {
    const id = recordingThis ? (note?.id ?? null) : null;
    if (id == null || id === coachFor) return;
    coachFor = id;
    invoke('notepad_coach_marks', { noteId: id })
      .then((m) => {
        if (note?.id === id) coach = { notes: !!m?.notes, catchUp: !!m?.catchUp };
      })
      .catch(() => {});
  });
  const showNotesMark = $derived(coach.notes && recordingThis && tab === 'thoughts');
  const showCatchUpMark = $derived(coach.catchUp && recordingThis && !askOpen);
  $effect(() => {
    for (const [kind, on] of [['notes', showNotesMark], ['catchUp', showCatchUpMark]]) {
      if (on && !counted.has(kind)) {
        counted.add(kind);
        invoke('notepad_coach_mark_shown', { kind }).catch(() => {});
      }
    }
  });
  $effect(() => {
    if (askOpen && coach.catchUp) coach.catchUp = false;
  });

  // ---- the Ask sheet: what the person has seen ----

  // The Transcript tab is up, showing its lines, at the newest one.
  const transcriptShown = $derived(
    tab === 'transcript' && view.followLog && !view.searchQuery && (liveTranscript || !recordingThis)
  );
  async function refreshShown() {
    try {
      windowShown = (await appWindow.isVisible()) && !(await appWindow.isMinimized());
    } catch {
      /* keep */
    }
  }
  // ▶ on an answer reads it aloud (speech.svelte.js, voice/tts.rs).
  initSpeech(['notepad:']);

  // ---- the popovers: one at a time ----

  function toggleMenu(which) {
    const next = menu === which ? null : which;
    closeMenus();
    menu = next;
  }
  /** Close whichever popover is open (saving an edited consent message). */
  function closeMenus() {
    if (menu === 'consent') consent.save();
    menu = null;
  }

  // ---- keys and clicks around the page ----

  function onWindowKeydown(e) {
    if (e.key !== 'Escape') return;
    // A right-click menu takes its own Esc (ui/contextMenu.svelte.js).
    if (e.defaultPrevented || menuOrDialogOpen()) return;
    if (confirmOpen) confirmOpen = false;
    else if (menu) closeMenus();
    else if (view.searchOpen) view.closeSearch();
    else if (askOpen) askTray?.close();
  }
  function onWindowPointerdown(e) {
    const t = e.target;
    // A click in a right-click menu or its dialog belongs to them.
    if (t.closest?.('[data-context-menu]') || t.closest?.('[role="alertdialog"]')) return;
    if (menu && !t.closest?.('.popwrap')) closeMenus();
    // A click above the sheet puts it away (not on a toast or the dialog).
    if (askOpen && !confirmOpen && !t.closest?.('.tray') && !t.closest?.('[role="status"]')) askTray?.close();
  }

  // ---- the in-page hotkey fallback (the WebView2-focus gotcha) ----

  async function refreshConfig() {
    try {
      cfg = await invoke('get_config');
      liveTranscript = cfg.meetingLiveTranscript !== false;
      consent.saved = cfg.meetingConsentMessage || '';
    } catch {
      /* keep */
    }
  }
  // The meeting shortcut (calls/meeting_guard.rs) goes first, as in Settings: a
  // combo is more specific than a bare dictation key; then the Voice
  // Assistant's (chat/assistant.rs: during a meeting it answers on the bar,
  // without speaking).
  function onKeyDown(e) {
    if (e.repeat || !cfg) return;
    // (Each action's more keys too: "+ Add another".)
    if (actionMatchesKeydown(e, cfg, 'meetingHotkey')) {
      e.preventDefault();
      e.stopPropagation();
      flushSave();
      invoke('meeting_shortcut', { origin: LABEL }).catch(() => {});
    } else if (actionMatchesKeydown(e, cfg, 'assistantHotkey')) {
      e.preventDefault();
      e.stopPropagation();
      invoke('assistant_key', { pressed: true }).catch(() => {});
    } else if (actionMatchesKeydown(e, cfg, 'readHotkey')) {
      // Read aloud (voice/read_aloud.rs): what's selected here. (During a meeting
      // Yap keeps quiet, and the card says so.)
      e.preventDefault();
      e.stopPropagation();
      readPageSelection();
    } else if (actionMatchesKeydown(e, cfg, 'quickNoteHotkey')) {
      // The quick note (notes/quick_note/key.rs) comes up in front; held,
      // a dictation into it.
      e.preventDefault();
      e.stopPropagation();
      flushSave();
      invoke('quick_note_key', { pressed: true }).catch(() => {});
    } else if (actionMatchesKeydown(e, cfg, 'hotkey')) {
      e.preventDefault();
      e.stopPropagation();
      invoke('dictation_key', { pressed: true }).catch(() => {});
    }
  }
  // The key coming up: push-to-talk's release (Rust decides by the mode).
  function onKeyUp(e) {
    if (!cfg) return;
    if (actionMatchesKeyup(e, cfg, 'quickNoteHotkey')) {
      e.preventDefault();
      invoke('quick_note_key', { pressed: false }).catch(() => {});
    } else if (actionMatchesKeyup(e, cfg, 'assistantHotkey')) {
      e.preventDefault();
      invoke('assistant_key', { pressed: false }).catch(() => {});
    } else if (actionMatchesKeyup(e, cfg, 'hotkey')) {
      e.preventDefault();
      invoke('dictation_key', { pressed: false }).catch(() => {});
    }
  }

  onMount(() => {
    initMeetingSummary();
    initMeetingAudio();
    // "Ending notes in 30 s…" and "Still in your meeting?" (calls/meeting_guard.rs),
    // here while the notepad is on screen.
    initMeetingCheckIns(LABEL);
    refreshConfig();
    refreshShown();
    window.addEventListener('keydown', onKeyDown, true);
    window.addEventListener('keyup', onKeyUp, true);
    const uns = [];
    const on = (name, fn) => listen(name, (e) => fn(e.payload)).then((u) => uns.push(u));
    invoke('notepad_state')
      .then((s) => load(s?.noteId ?? null))
      .catch(() => {});
    invoke('meeting_state')
      .then((s) => {
        meeting = s || { recording: false };
        if (meeting.recording) startElapsed(meeting.elapsedSecs || 0);
      })
      .catch(() => {});
    on('yap-notepad-note', (p) => load(p?.noteId ?? null));
    on('yap-notepad-visible', (shown) => {
      windowShown = !!shown;
      refreshShown();
    });
    on('yap-meeting-state', onMeetingState);
    on('yap-meeting-segment', onSegment);
    on('yap-meeting-digest', onDigest);
    on('yap-meeting-breaks', onBreaks);
    on('yap-meeting-summary', onSummary);
    on('yap-note-changed', onNoteChanged);
    on('yap-meeting-brief', onBrief);
    on('yap-meeting-speakers', onSpeakers);
    on('yap-note-deleted', (p) => p?.id === note?.id && (note = null));
    on('yap-notes-changed', () => note && reload());
    // "Started by mistake?", when Rust picked this window to ask.
    on('yap-meeting-ended', (p) => {
      if (p?.mistake && p.surface === LABEL) askStartedByMistake(p.noteId);
    });
    appWindow
      .onFocusChanged(({ payload: focused }) => {
        refreshShown();
        if (focused) refreshConfig();
      })
      .then((u) => uns.push(u));
    // WebView2 reports the page visible while the window is hidden, so ask
    // the window whether it's on screen (for what counts as seen).
    const poll = setInterval(refreshShown, 1500);
    return () => {
      flushSave();
      stopElapsed();
      clearInterval(poll);
      window.removeEventListener('keydown', onKeyDown, true);
      window.removeEventListener('keyup', onKeyUp, true);
      uns.forEach((u) => u && u());
    };
  });
</script>

<svelte:window onkeydown={onWindowKeydown} onpointerdown={onWindowPointerdown} />

<div class="pad">
  <NotepadHeader
    {note}
    {recordingThis}
    {lineCount}
    bind:menu
    {consent}
    ontogglemenu={toggleMenu}
    onflushsave={flushSave}
    onaskdelete={() => (confirmOpen = true)}
  />

  {#if !note}
    <div class="nonote">
      <span class="noico"><Icon name="note" /></span>
      <p class="n1">No meeting open</p>
      <p class="n2">The notepad opens when a meeting starts recording. Reopen it from a meeting note in Yap.</p>
    </div>
  {:else}
    <NotepadTitle bind:note onsave={() => queueSave('title')} />
    <NotepadTabs bind:tab {recordingThis} {hasSummary} />

    {#key tab}
      <main class="body">
        {#if tab === 'thoughts'}
          {#if showNotesMark}
            <div class="coachtop">
              <CoachMark
                title="Take notes here"
                body="Type what matters to you. Yap's summary builds on your notes and the transcript."
                ondismiss={() => (coach.notes = false)}
              />
            </div>
          {/if}
          <ThoughtsTab
            bind:note
            {recordingThis}
            {brief}
            bind:briefLineGone
            onsave={() => {
              coach.notes = false;
              queueSave('content');
            }}
            onopenbrief={openBrief}
          />
        {:else if tab === 'transcript'}
          <TranscriptTab
            bind:note
            {view}
            {recordingThis}
            {elapsed}
            {liveTranscript}
            {speakersNote}
            {changing}
            onreload={reload}
            onspeakerchanged={onSpeakerChanged}
          />
        {:else if tab === 'tasks'}
          <MeetingTasks noteId={note.id} look="paper" />
        {:else}
          <SummaryTab
            bind:note
            {recordingThis}
            {job}
            {progress}
            {writing}
            {hasSummary}
            {brief}
            bind:briefFold
            {summary}
            {changing}
            ongenerate={generate}
            onspeakerchanged={onSpeakerChanged}
          />
        {/if}
      </main>
    {/key}

    <div class="bottom">
      <div class="fade" aria-hidden="true"></div>
      <GenerateSummary {note} {job} {recordingThis} {askOpen} {writing} {hasSummary} ongenerate={generate} />
      {#if recordingThis && !askOpen}
        <ConsentLine bind:menu {consent} ontogglemenu={toggleMenu} />
      {/if}
      {#if showCatchUpMark}
        <div class="coachbottom">
          <CoachMark
            title="Try What did I miss?"
            body="Looked away? It catches you up on what was said since you last read the transcript."
            pointer="down"
            align="right"
            ondismiss={() => (coach.catchUp = false)}
          />
        </div>
      {/if}
      <AskTray
        bind:this={askTray}
        bind:open={askOpen}
        {note}
        {recordingThis}
        meetingRecording={meeting.recording}
        {stopping}
        {transcriptShown}
        {windowShown}
        onstop={stop}
        onresume={resume}
        onsource={openSource}
        onshowsummary={() => (tab = 'summary')}
      />
    </div>

    <DeleteMeetingDialog bind:open={confirmOpen} noteId={note.id} {recordingThis} />
  {/if}

  <ToastHost />
</div>

<style>
  .pad {
    /* Toasts sit above the bar in this narrow window. */
    --yap-toast-bottom: 104px;
    --yap-toast-right: 20px;
    --them: rgb(46 106 134);
    position: relative;
    display: flex;
    flex-direction: column;
    height: 100vh;
    background: var(--yap-paper);
    color: var(--yap-paper-ink);
    font-size: 15px;
    overflow: hidden;
  }
  /* "Them" in a lighter teal on the dark paper (7.2:1). */
  :global(:root[data-yap-theme='dark']) .pad {
    --them: rgb(110 175 204);
  }
  .pad :global(svg) {
    flex: 0 0 auto;
  }

  /* ---- the tab bodies (each tab is its own component) ---- */
  .body {
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
    animation: bodyin 0.18s ease-out;
  }
  @keyframes bodyin {
    from {
      opacity: 0;
    }
  }

  /* ---- the bottom: fade, Generate summary, consent, the bar ---- */
  .bottom {
    position: absolute;
    left: 20px;
    right: 20px;
    bottom: 0;
    z-index: 20;
    pointer-events: none;
  }
  .fade {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    height: 150px;
    background: linear-gradient(to top, var(--yap-paper) calc(100% - 45px), transparent);
  }
  /* The first-meeting coach marks: under the tabs, and over the Ask bar's
     chip (above the consent line). */
  .coachtop {
    flex: 0 0 auto;
    padding: 12px 20px 0;
  }
  .coachbottom {
    position: absolute;
    right: 6px;
    bottom: 118px;
    display: flex;
    justify-content: flex-end;
  }

  /* ---- no note ---- */
  .nonote {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding: 0 40px 60px;
    text-align: center;
    color: var(--yap-paper-muted);
  }
  .noico {
    display: inline-flex;
    color: var(--yap-paper-faint);
  }
  .noico :global(svg) {
    width: 32px;
    height: 32px;
    stroke-width: 1.5;
  }
  .n1 {
    margin: 4px 0 0;
    font-family: var(--yap-font-display);
    font-size: 24px;
    color: var(--yap-paper-ink);
  }
  .n2 {
    margin: 0;
    font-size: 13px;
    line-height: 20px;
  }
</style>
