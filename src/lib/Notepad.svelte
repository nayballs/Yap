<script>
  // The meeting notepad (notepad.rs) — Yap's meeting UI while a call runs,
  // matched to Wispr Flow's Notetaker notepad from measurements of its DOM
  // (sizes, type, colours and motion; the palette is `--yap-paper-*` in
  // app.css). Top to bottom:
  //   - the header: back (this note in Yap's Notes), ⋯ (copy as Markdown or
  //     text, copy the consent message, audio settings while recording, save
  //     as .md, delete), split the screen with the call (a glass preview on
  //     hover), Share (a local popover: copy, save, email) with a one-click
  //     copy, and the window buttons;
  //   - the title (EB Garamond; a made-up one waits, muted, for the AI title)
  //     and the date in the locale's format;
  //   - tabs: My thoughts (the note's own text, synced with the Notes view
  //     both ways: `yap-note-changed`, `origin` = window label), Transcript (a
  //     live waveform on the tab while recording; the timer box with search
  //     and copy; speaker groups of chat bubbles with a hover copy; a paused
  //     divider where a recording stopped, `Note::breaks`; a marker where the
  //     person dictated mid-meeting, a `dictated` segment) and + Summary (the
  //     action plan Rust writes when the meeting ends, meeting_end.rs, with
  //     "Step 2 of 3");
  //   - the bar: Stop / Resume, and "Ask anything", which opens a bottom
  //     sheet chat grounded in the meeting (meeting_assist.rs; "What did I
  //     miss?" answers from what was said since you last looked: the
  //     transcript on screen at its newest line, or the last answer). Above
  //     it while recording, the consent line, whose "Learn more" offers a
  //     message for the meeting chat (editable, `meetingConsentMessage`);
  //     after the meeting, Generate summary when there's none, or the
  //     summary's error with a Retry.
  // Like every Yap window you can type in, it catches the dictation key and
  // the meeting shortcut in-page (a focused WebView2 window never reaches the
  // global hook).
  import { invoke } from '@tauri-apps/api/core';
  import { emit, listen } from '@tauri-apps/api/event';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { onMount, tick } from 'svelte';
  import ToastHost from './ui/ToastHost.svelte';
  import { toast } from './ui/toast.svelte.js';
  import { renderMarkdown } from './markdown.js';
  import { hotkeyMatchesKeydown, hotkeyMatchesKeyup } from './hotkeys.js';
  import { openExternalLink } from './externalLinks.js';
  import {
    summaries,
    initMeetingSummary,
    loadSummary,
    summarise,
    stepLine,
    askStartedByMistake,
  } from './meetingSummary.svelte.js';
  import {
    noteMarkdown,
    noteText,
    transcriptText,
    mailtoUrl,
    transcriptItems,
    bubbleShape,
    highlightParts,
    dateLine,
    clock,
    consentMessage,
    consentToSave,
  } from './notepadText.js';

  /** This window's label: the `origin` of its saves and stops. */
  const LABEL = 'notepad';
  const appWindow = getCurrentWindow();

  let note = $state(null); // the meeting note shown (full)
  let meeting = $state({ recording: false });
  let elapsed = $state(0);
  let elapsedTimer = null;
  let stopping = $state(false);
  let tab = $state('thoughts'); // thoughts | transcript | summary
  let cfg = null; // for the in-page hotkey fallback
  // "Show live transcript" (Settings → General → Meetings): off, the
  // Transcript tab stays quiet until the meeting stops.
  let liveTranscript = $state(true);
  let maximized = $state(false);

  const recordingThis = $derived(!!note && meeting.recording && meeting.noteId === note.id);
  const job = $derived(note ? summaries.byNote[note.id] : null);
  const progress = $derived(note ? summaries.progress[note.id] : null);
  const writing = $derived(job?.state === 'running');
  const hasSummary = $derived(!!note?.enhancedContent?.trim());

  // ---- loading and saving ----

  async function load(id) {
    flushSave();
    if (id == null) {
      note = null;
      return;
    }
    try {
      const n = await invoke('note_get', { id });
      if (note?.id !== n.id) {
        tab = 'thoughts';
        askOpen = false;
        askThread = [];
        askSetup = null;
        showEcho = false;
        followLog = true;
        searchOpen = false;
        query = '';
        menu = null;
        confirmOpen = false;
      }
      note = n;
      loadSummary(n.id);
    } catch {
      note = null;
    }
  }

  async function reload() {
    if (!note) return;
    try {
      const n = await invoke('note_get', { id: note.id });
      if (note?.id !== n.id) return;
      // Keep what's being typed; take the rest.
      if (!dirty.title) {
        note.title = n.title;
        note.titleAuto = n.titleAuto;
      }
      if (!dirty.content) note.content = n.content;
      note.transcript = n.transcript;
      note.breaks = n.breaks;
      note.digests = n.digests;
      note.enhancedContent = n.enhancedContent;
      note.participants = n.participants;
    } catch {
      note = null; // gone (deleted)
    }
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
    invoke('note_update', args).catch(() => {});
  }

  // Another window (or the AI title) changed the note.
  function onNoteChanged(p) {
    if (!note || p?.id !== note.id || p.origin === LABEL) return;
    if (!dirty.title) {
      note.title = p.title;
      note.titleAuto = p.titleAuto;
    }
    if (!dirty.content) note.content = p.content;
    note.participants = p.participants;
  }

  // The title grows with what it says (a textarea, like Wispr's), one line
  // per line; Enter finishes it.
  let titleEl = $state(null);
  function fitTitle() {
    if (!titleEl) return;
    titleEl.style.height = 'auto';
    titleEl.style.height = `${titleEl.scrollHeight}px`;
  }
  $effect(() => {
    void note?.title;
    void titleEl;
    tick().then(fitTitle);
  });
  function onTitleKeydown(e) {
    if (e.key === 'Enter') {
      e.preventDefault();
      e.currentTarget.blur();
    }
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
    } else {
      stopElapsed();
      stopping = false;
      // The last chunk is in: the transcript as stored.
      if (was && note && wasNote === note.id) reload();
    }
  }

  function onSegment(seg) {
    if (!seg || !recordingThis) return;
    note.transcript = [...(note.transcript || []), seg];
  }

  function onDigest(p) {
    if (p?.digest && note?.id === p.noteId) note.digests = [...(note.digests || []), p.digest];
  }

  // A recording stopped: where, for the paused divider.
  function onBreaks(p) {
    if (note && p?.noteId === note.id) note.breaks = p.breaks;
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

  async function generate() {
    if (!note || writing) return;
    flushSave();
    tab = 'summary';
    try {
      await summarise(note.id);
    } catch (e) {
      toast({ title: "Couldn't write the summary", description: String(e), variant: 'destructive' });
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

  // ---- the transcript ----

  let showEcho = $state(false);
  let searchOpen = $state(false);
  let query = $state('');
  let searchEl = $state(null);
  const searchQuery = $derived(searchOpen ? query.trim() : '');
  const items = $derived(
    transcriptItems(note?.transcript, note?.breaks, { showEcho, query: searchQuery })
  );
  const echoCount = $derived((note?.transcript || []).filter((s) => s.echo).length);
  const lineCount = $derived((note?.transcript || []).length);
  const meetingSecs = $derived.by(() => {
    const t = (note?.transcript || []).filter((s) => !s.dictated);
    return t.length > 1 ? t[t.length - 1].ts - t[0].ts : 0;
  });
  const minutes = $derived(note?.transcript?.length ? Math.round(meetingSecs / 60) : null);

  async function toggleSearch() {
    if (searchOpen) {
      closeSearch();
      return;
    }
    searchOpen = true;
    await tick();
    searchEl?.focus();
  }
  function closeSearch() {
    searchOpen = false;
    query = '';
  }

  // The tip about the live transcript, dismissible for good.
  let hintGone = $state(readFlag('yapNotepadHintGone'));
  function readFlag(key) {
    try {
      return localStorage.getItem(key) === '1';
    } catch {
      return false;
    }
  }
  function dismissHint() {
    hintGone = true;
    try {
      localStorage.setItem('yapNotepadHintGone', '1');
    } catch {
      /* private storage: just this session */
    }
  }

  // Follow the newest line unless the person scrolled up to read.
  let logEl = $state(null);
  let followLog = $state(true);
  $effect(() => {
    void items.length;
    void lineCount;
    if (logEl && followLog && !searchQuery) logEl.scrollTop = logEl.scrollHeight;
  });

  // Copy: one line (from its bubble), or the whole transcript.
  let copiedLine = $state(null);
  let transcriptCopied = $state(false);
  async function copyLine(line) {
    if (await copyToClipboard(line.text)) {
      copiedLine = line.i;
      setTimeout(() => copiedLine === line.i && (copiedLine = null), 1200);
    }
  }
  async function copyTranscript() {
    if (await copyToClipboard(transcriptText(note?.transcript))) {
      transcriptCopied = true;
      setTimeout(() => (transcriptCopied = false), 1200);
      toast({ title: 'Transcript copied', variant: 'success' });
    }
  }
  async function copyToClipboard(text) {
    try {
      await navigator.clipboard.writeText(text);
      return true;
    } catch {
      toast({ title: "Couldn't copy", description: 'The clipboard is busy. Try again.', variant: 'destructive' });
      return false;
    }
  }

  // ---- the Ask sheet: "What did I miss?" and questions about the meeting ----

  // What the person has seen, per note (transcript segments): everything,
  // whenever the transcript is on screen and scrolled to its newest line.
  const seen = new Map();
  let windowShown = $state(true);
  /** The Transcript tab is up, showing its lines, at the newest one. */
  function transcriptShown() {
    return tab === 'transcript' && followLog && !searchQuery && (liveTranscript || !recordingThis);
  }
  async function refreshShown() {
    try {
      windowShown = (await appWindow.isVisible()) && !(await appWindow.isMinimized());
    } catch {
      /* keep */
    }
  }
  $effect(() => {
    const n = lineCount;
    if (note && transcriptShown() && windowShown && document.visibilityState === 'visible') {
      seen.set(note.id, Math.max(seen.get(note.id) ?? 0, n));
    }
  });

  let askOpen = $state(false);
  let askThread = $state([]); // { id, role: 'user' | 'assistant', text, error? }
  let askBusy = $state(false);
  let askSetup = $state(null); // the "set up an AI model" reason
  let askInput = $state('');
  let askFocused = $state(false);
  let askEl = $state(null);
  let threadEl = $state(null);
  let askSeq = 0;

  function openAsk() {
    if (askOpen) return;
    askOpen = true;
    scrollThread();
  }
  function closeAsk() {
    askOpen = false;
    askEl?.blur();
  }
  function newChat() {
    askThread = [];
    askSetup = null;
    askInput = '';
    fitAsk();
    askEl?.focus();
  }

  /** Ask: no `question` = "What did I miss?" (since you last looked). */
  async function ask(question = null) {
    if (!note || askBusy) return;
    const id = note.id;
    const q = question?.trim() || null;
    // Asking with the transcript on screen, at its newest line: that's seen.
    if (!q && transcriptShown()) {
      seen.set(id, Math.max(seen.get(id) ?? 0, lineCount));
    }
    openAsk();
    askSetup = null;
    askThread = [...askThread, { id: `${Date.now()}-${++askSeq}`, role: 'user', text: q ?? 'What did I miss?' }];
    askBusy = true;
    scrollThread();
    try {
      const r = await invoke('meeting_catch_up', { noteId: id, since: seen.get(id) ?? 0, question: q });
      if (note?.id !== id) return;
      askThread = [...askThread, { id: `${Date.now()}-${++askSeq}`, role: 'assistant', text: r.answer }];
      if (!q) seen.set(id, Math.max(seen.get(id) ?? 0, r.upto));
    } catch (e) {
      const msg = String(e);
      if (/No AI model configured|API key/i.test(msg)) askSetup = msg;
      else
        askThread = [
          ...askThread,
          { id: `${Date.now()}-${++askSeq}`, role: 'assistant', text: `I couldn't answer that: ${msg}`, error: true },
        ];
    } finally {
      askBusy = false;
      scrollThread();
    }
  }
  function sendTyped() {
    const q = askInput.trim();
    if (!q || askBusy) return;
    askInput = '';
    fitAsk();
    ask(q);
  }
  function onAskKeydown(e) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      sendTyped();
    }
  }
  function fitAsk() {
    tick().then(() => {
      if (!askEl) return;
      askEl.style.height = 'auto';
      askEl.style.height = `${Math.min(askEl.scrollHeight, 110)}px`;
    });
  }
  async function scrollThread() {
    await tick();
    if (threadEl) threadEl.scrollTop = threadEl.scrollHeight;
  }

  // 👍 / 👎 on an answer: kept on this PC only, never sent anywhere.
  const FEEDBACK_KEY = 'yapNotepadFeedback';
  let feedback = $state(readFeedback());
  function readFeedback() {
    try {
      return JSON.parse(localStorage.getItem(FEEDBACK_KEY) || '{}') || {};
    } catch {
      return {};
    }
  }
  function rate(answer, rating) {
    const next = { ...feedback };
    if (next[answer.id]?.rating === rating) delete next[answer.id];
    else next[answer.id] = { rating, noteId: note?.id, at: Date.now() };
    const keep = Object.entries(next).sort((a, b) => b[1].at - a[1].at).slice(0, 200);
    feedback = Object.fromEntries(keep);
    try {
      localStorage.setItem(FEEDBACK_KEY, JSON.stringify(feedback));
    } catch {
      /* private storage: just this session */
    }
  }
  let copiedAnswer = $state(null);
  async function copyAnswer(answer) {
    if (await copyToClipboard(answer.text)) {
      copiedAnswer = answer.id;
      setTimeout(() => copiedAnswer === answer.id && (copiedAnswer = null), 1200);
    }
  }

  // ---- the header: menus, share, split, window ----

  let menu = $state(null); // 'more' | 'share' | 'consent' | null
  let confirmOpen = $state(false);
  let deleting = $state(false);
  let linkCopied = $state(false);

  function toggleMenu(which) {
    const next = menu === which ? null : which;
    closeMenus();
    menu = next;
  }
  /** Close whichever popover is open (saving an edited consent message). */
  function closeMenus() {
    if (menu === 'consent') saveConsent();
    menu = null;
  }

  // The consent message: "Learn more" on the consent line opens it, why and
  // a message for the meeting chat, editable (saved on blur, empty = Yap's
  // default); the ⋯ menu copies the saved one.
  let savedConsent = $state(''); // as saved ('' = the default)
  let consentDraft = $state('');
  function openConsent() {
    if (menu !== 'consent') consentDraft = consentMessage(savedConsent);
    toggleMenu('consent');
  }
  async function saveConsent() {
    const message = consentToSave(consentDraft);
    if (message === savedConsent) return;
    savedConsent = message;
    if (cfg) cfg.meetingConsentMessage = message;
    try {
      await invoke('notepad_consent_message', { message });
    } catch (e) {
      toast({ title: "Couldn't save the message", description: String(e), variant: 'destructive' });
    }
  }
  /** Copy the message being edited, or (from the ⋯ menu) the saved one. */
  async function copyConsent(saved) {
    let text;
    if (saved) {
      menu = null;
      text = consentMessage(savedConsent);
    } else {
      await saveConsent();
      text = consentMessage(consentDraft);
    }
    if (await copyToClipboard(text)) {
      toast({ title: 'Copied: paste it into the meeting chat', variant: 'success' });
    }
  }

  async function copyNote(asText) {
    menu = null;
    flushSave();
    if (await copyToClipboard(asText ? noteText(note) : noteMarkdown(note))) {
      toast({ title: asText ? 'Notes copied as text' : 'Notes copied as Markdown', variant: 'success' });
    }
  }
  // The link half of Share: the notes, copied in one click.
  async function copyLink() {
    flushSave();
    if (await copyToClipboard(noteMarkdown(note))) {
      linkCopied = true;
      setTimeout(() => (linkCopied = false), 1500);
    }
  }
  async function saveNote() {
    menu = null;
    flushSave();
    try {
      const { save } = await import('@tauri-apps/plugin-dialog');
      const name = (note.title?.trim() || 'Meeting notes').replace(/[\\/:*?"<>|]/g, '-');
      const path = await save({ defaultPath: `${name}.md`, filters: [{ name: 'Markdown', extensions: ['md'] }] });
      if (!path) return;
      await invoke('note_export', { id: note.id, path });
      toast({ title: 'Notes saved', description: path, variant: 'success' });
    } catch (e) {
      toast({ title: "Couldn't save the notes", description: String(e), variant: 'destructive' });
    }
  }
  function emailNote() {
    menu = null;
    flushSave();
    openExternalLink(mailtoUrl(note));
  }
  function audioSettings() {
    menu = null;
    openSettings('general');
  }
  function askDelete() {
    menu = null;
    confirmOpen = true;
  }
  async function deleteNote() {
    if (!note || deleting) return;
    deleting = true;
    try {
      // The notepad lets go of the note and hides (`yap-note-deleted`).
      await invoke('meeting_delete', { noteId: note.id });
      confirmOpen = false;
    } catch (e) {
      toast({ title: "Couldn't delete the meeting", description: String(e), variant: 'destructive' });
    } finally {
      deleting = false;
    }
  }

  // Split the screen with the call now; hovering shows where it would go.
  function previewSplit(show) {
    invoke('notepad_split_preview', { show }).catch(() => {});
  }
  async function splitScreen() {
    previewSplit(false);
    try {
      await invoke('notepad_split');
    } catch (e) {
      toast({ title: "Couldn't split the screen", description: String(e) });
    }
  }

  async function refreshMaximized() {
    try {
      maximized = await appWindow.isMaximized();
    } catch {
      /* keep */
    }
  }

  // ---- elsewhere in Yap ----

  /** Show Yap's main window at a Settings section ('cleanup' = Language Models). */
  async function openSettings(section) {
    await invoke('open_settings').catch(() => {});
    emit('yap-open-settings', section).catch(() => {});
  }
  /** Back: the note in the main window's Notes view. */
  async function openInYap() {
    if (!note) return;
    flushSave();
    await invoke('open_settings').catch(() => {});
    emit('yap-meeting-open-note', { noteId: note.id, stop: false }).catch(() => {});
  }

  // ---- keys and clicks around the page ----

  function onWindowKeydown(e) {
    if (e.key !== 'Escape') return;
    if (confirmOpen) confirmOpen = false;
    else if (menu) closeMenus();
    else if (searchOpen) closeSearch();
    else if (askOpen) closeAsk();
  }
  function onWindowPointerdown(e) {
    const t = e.target;
    if (menu && !t.closest?.('.popwrap')) closeMenus();
    // A click above the sheet puts it away (not on a toast or the dialog).
    if (askOpen && !confirmOpen && !t.closest?.('.tray') && !t.closest?.('[role="status"]')) closeAsk();
  }

  // ---- the in-page hotkey fallback (the WebView2-focus gotcha) ----

  async function refreshConfig() {
    try {
      cfg = await invoke('get_config');
      liveTranscript = cfg.meetingLiveTranscript !== false;
      savedConsent = cfg.meetingConsentMessage || '';
    } catch {
      /* keep */
    }
  }
  // The meeting shortcut (meeting_guard.rs) goes first, as in Settings: a
  // combo is more specific than a bare dictation key.
  function onKeyDown(e) {
    if (e.repeat || !cfg) return;
    if (cfg.meetingHotkey && hotkeyMatchesKeydown(e, cfg.meetingHotkey)) {
      e.preventDefault();
      e.stopPropagation();
      flushSave();
      invoke('meeting_shortcut', { origin: LABEL }).catch(() => {});
    } else if (hotkeyMatchesKeydown(e, cfg.hotkey)) {
      e.preventDefault();
      e.stopPropagation();
      invoke('toggle_recording').catch(() => {});
    }
  }
  function onKeyUp(e) {
    if (!cfg || cfg.recordingMode !== 'pushToTalk') return;
    if (hotkeyMatchesKeyup(e, cfg.hotkey)) {
      e.preventDefault();
      invoke('toggle_recording').catch(() => {});
    }
  }

  onMount(() => {
    initMeetingSummary();
    refreshConfig();
    refreshShown();
    refreshMaximized();
    window.addEventListener('keydown', onKeyDown, true);
    window.addEventListener('keyup', onKeyUp, true);
    const onResize = () => fitTitle();
    window.addEventListener('resize', onResize);
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
    on('yap-note-deleted', (p) => p?.id === note?.id && (note = null));
    on('yap-notes-changed', () => note && reload());
    // Something about the recording itself (meeting.rs), as the main window
    // shows it (meetingGuard.js); Rust adds a Windows notification only when
    // neither window is in view.
    on('yap-meeting-warning', (msg) =>
      toast({ title: 'Meeting recording', description: String(msg), chip: 'Tip', duration: 12_000 })
    );
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
    appWindow.onResized(() => refreshMaximized()).then((u) => uns.push(u));
    // WebView2 reports the page visible while the window is hidden, so ask
    // the window whether it's on screen (for what counts as seen).
    const poll = setInterval(refreshShown, 1500);
    return () => {
      flushSave();
      stopElapsed();
      clearInterval(poll);
      previewSplit(false);
      window.removeEventListener('keydown', onKeyDown, true);
      window.removeEventListener('keyup', onKeyUp, true);
      window.removeEventListener('resize', onResize);
      uns.forEach((u) => u && u());
    };
  });
</script>

<!-- Yap's own icons (stroke style), sized by the CSS of where they sit. -->
{#snippet icon(name)}
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    {#if name === 'back'}<path d="M15 18l-6-6 6-6" />
    {:else if name === 'more'}<circle cx="5" cy="12" r="1.3" fill="currentColor" stroke="none" /><circle cx="12" cy="12" r="1.3" fill="currentColor" stroke="none" /><circle cx="19" cy="12" r="1.3" fill="currentColor" stroke="none" />
    {:else if name === 'split'}<rect x="3.5" y="4.5" width="17" height="15" rx="2.5" /><path d="M12 4.5v15" />
    {:else if name === 'share'}<path d="M12 15V3.5" /><path d="M7.5 8L12 3.5 16.5 8" /><path d="M5 12.5v5.5a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-5.5" />
    {:else if name === 'link'}<path d="M10 14a4.5 4.5 0 0 0 6.4 0l2.8-2.8a4.5 4.5 0 0 0-6.4-6.4l-1.2 1.2" /><path d="M14 10a4.5 4.5 0 0 0-6.4 0l-2.8 2.8a4.5 4.5 0 0 0 6.4 6.4l1.2-1.2" />
    {:else if name === 'check'}<path d="M5 12.5l4.5 4.5L19 7.5" />
    {:else if name === 'min'}<path d="M5 12h14" />
    {:else if name === 'max'}<rect x="5" y="5" width="14" height="14" rx="1.5" />
    {:else if name === 'restore'}<path d="M8 8V5.5a1 1 0 0 1 1-1h9.5a1 1 0 0 1 1 1V15a1 1 0 0 1-1 1H16" /><rect x="4.5" y="8" width="11.5" height="11.5" rx="1" />
    {:else if name === 'close'}<path d="M6 6l12 12M18 6L6 18" />
    {:else if name === 'clock'}<circle cx="12" cy="12" r="8.5" /><path d="M12 7.5V12l3 2" />
    {:else if name === 'search'}<circle cx="11" cy="11" r="6" /><path d="M20 20l-4.5-4.5" />
    {:else if name === 'copy'}<rect x="8.5" y="8.5" width="11" height="11" rx="2.5" /><path d="M15.5 8.5V6.5a2 2 0 0 0-2-2h-7a2 2 0 0 0-2 2v7a2 2 0 0 0 2 2h2" />
    {:else if name === 'x'}<path d="M7 7l10 10M17 7L7 17" />
    {:else if name === 'pause'}<rect x="7" y="5" width="3.2" height="14" rx="1" /><rect x="13.8" y="5" width="3.2" height="14" rx="1" />
    {:else if name === 'mic'}<rect x="9" y="3" width="6" height="11" rx="3" /><path d="M5.5 11a6.5 6.5 0 0 0 13 0" /><path d="M12 17.5V21" />
    {:else if name === 'plus'}<path d="M12 5v14M5 12h14" />
    {:else if name === 'markdown'}<path d="M14 3.5H7a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8.5z" /><path d="M14 3.5v5h5" /><path d="M9 13.5h6M9 17h4" />
    {:else if name === 'text'}<path d="M5 6.5h14M5 12h14M5 17.5h9" />
    {:else if name === 'audio'}<rect x="9" y="3" width="6" height="10" rx="3" /><path d="M5.5 10.5a6.5 6.5 0 0 0 13 0" /><path d="M12 17v3.5" /><path d="M8.5 20.5h7" />
    {:else if name === 'save'}<path d="M12 4v11" /><path d="M7.5 10.5L12 15l4.5-4.5" /><path d="M5 19.5h14" />
    {:else if name === 'mail'}<rect x="3.5" y="5.5" width="17" height="13" rx="2" /><path d="M4 7l8 6 8-6" />
    {:else if name === 'trash'}<path d="M4.5 7h15" /><path d="M9.5 7V5a1 1 0 0 1 1-1h3a1 1 0 0 1 1 1v2" /><path d="M6.5 7l.8 12a1.5 1.5 0 0 0 1.5 1.4h6.4a1.5 1.5 0 0 0 1.5-1.4l.8-12" />
    {:else if name === 'newchat'}<path d="M12 4.5H6.5a2 2 0 0 0-2 2v11a2 2 0 0 0 2 2h11a2 2 0 0 0 2-2V12" /><path d="M17.5 3.5a2.1 2.1 0 0 1 3 3L13 14l-4 1 1-4z" />
    {:else if name === 'up'}<path d="M7.5 10.5v9H5a1 1 0 0 1-1-1v-7a1 1 0 0 1 1-1z" /><path d="M7.5 10.5l3.6-6.3a1.6 1.6 0 0 1 2.9 1.2l-.8 3.6h5.1a2 2 0 0 1 2 2.3l-1.1 6.4a2 2 0 0 1-2 1.7H7.5" />
    {:else if name === 'down'}<path d="M7.5 13.5v-9H5a1 1 0 0 0-1 1v7a1 1 0 0 0 1 1z" /><path d="M7.5 13.5l3.6 6.3a1.6 1.6 0 0 0 2.9-1.2l-.8-3.6h5.1a2 2 0 0 0 2-2.3l-1.1-6.4a2 2 0 0 0-2-1.7H7.5" />
    {:else if name === 'send'}<path d="M12 19V5.5" /><path d="M6.5 11L12 5.5 17.5 11" />
    {:else if name === 'warn'}<path d="M12 9.5v4" /><path d="M12 17h.01" /><path d="M10.3 4.2L2.6 17.5a2 2 0 0 0 1.7 3h15.4a2 2 0 0 0 1.7-3L13.7 4.2a2 2 0 0 0-3.4 0z" />
    {:else if name === 'retry'}<path d="M19.5 12a7.5 7.5 0 1 1-2.2-5.3" /><path d="M19.5 4.5V9H15" />
    {:else if name === 'note'}<path d="M14 3.5H7a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8.5z" /><path d="M14 3.5v5h5" />
    {:else if name === 'chat'}<path d="M20 11.5a7.5 7.5 0 0 1-10.9 6.7L4.5 19.5l1.3-4.3A7.5 7.5 0 1 1 20 11.5z" />
    {/if}
  </svg>
{/snippet}

<!-- Stop is a square, Start / Resume a ringed dot (the recording light). -->
{#snippet recIcon(stopIt)}
  <svg viewBox="0 0 16 16" aria-hidden="true">
    {#if stopIt}
      <rect x="2.5" y="2.5" width="11" height="11" rx="2.5" fill="currentColor" />
    {:else}
      <circle cx="8" cy="8" r="6.25" fill="none" stroke="currentColor" stroke-width="1.5" />
      <circle cx="8" cy="8" r="3.25" fill="currentColor" />
    {/if}
  </svg>
{/snippet}

<svelte:window onkeydown={onWindowKeydown} onpointerdown={onWindowPointerdown} />

<div class="pad">
  <header class="hdr" data-tauri-drag-region>
    {#if note}
      <button class="iconbtn back tipstart" aria-label="Open in Yap" data-tip="Open in Yap" onclick={openInYap}>
        {@render icon('back')}
      </button>
    {/if}
    <span class="hdrspace" data-tauri-drag-region></span>
    <div class="hdrright">
      {#if note}
        <div class="popwrap">
          <button
            class="iconbtn"
            class:open={menu === 'more'}
            aria-label="More"
            aria-haspopup="menu"
            aria-expanded={menu === 'more'}
            data-tip={menu === 'more' ? null : 'More'}
            onclick={() => toggleMenu('more')}
          >
            {@render icon('more')}
          </button>
          {#if menu === 'more'}
            <div class="pop menu" role="menu" aria-label="More">
              <button role="menuitem" class="item" onclick={() => copyNote(false)}>
                {@render icon('markdown')}<span>Copy as Markdown</span>
              </button>
              <button role="menuitem" class="item" onclick={() => copyNote(true)}>
                {@render icon('text')}<span>Copy as text</span>
              </button>
              <button role="menuitem" class="item" onclick={() => copyConsent(true)}>
                {@render icon('chat')}<span>Copy consent message</span>
              </button>
              {#if recordingThis}
                <button role="menuitem" class="item" onclick={audioSettings}>
                  {@render icon('audio')}<span>Audio settings</span>
                </button>
              {/if}
              <button role="menuitem" class="item" onclick={saveNote}>
                {@render icon('save')}<span>Save as .md</span>
              </button>
              <button role="menuitem" class="item danger" onclick={askDelete}>
                {@render icon('trash')}<span>Delete</span>
              </button>
            </div>
          {/if}
        </div>
      {/if}
      <button
        class="iconbtn"
        aria-label="Split screen with meeting"
        data-tip="Split screen with meeting"
        onclick={splitScreen}
        onpointerenter={() => previewSplit(true)}
        onpointerleave={() => previewSplit(false)}
      >
        {@render icon('split')}
      </button>
      {#if note}
        <div class="popwrap">
          <div class="sharegroup">
            <button
              class="sharebtn"
              class:open={menu === 'share'}
              aria-haspopup="dialog"
              aria-expanded={menu === 'share'}
              onclick={() => toggleMenu('share')}
            >
              {@render icon('share')}Share
            </button>
            <button class="linkbtn" aria-label="Copy notes" data-tip={linkCopied ? 'Copied' : 'Copy notes'} onclick={copyLink}>
              <span class="swap" class:on={linkCopied}>
                <span class="a">{@render icon('link')}</span>
                <span class="b">{@render icon('check')}</span>
              </span>
            </button>
          </div>
          {#if menu === 'share'}
            <div class="pop share" role="dialog" aria-label="Share notes">
              <p class="pophead">Share notes</p>
              <p class="popsub">Your notes stay on this PC. Copy them, save a file or start an email.</p>
              <button class="item" onclick={() => copyNote(false)}>{@render icon('markdown')}<span>Copy as Markdown</span></button>
              <button class="item" onclick={() => copyNote(true)}>{@render icon('text')}<span>Copy as text</span></button>
              <button class="item" onclick={saveNote}>{@render icon('save')}<span>Save as .md</span></button>
              <button class="item" onclick={emailNote}>{@render icon('mail')}<span>Email</span></button>
            </div>
          {/if}
        </div>
      {/if}
      <div class="winbtns">
        <button class="winbtn" aria-label="Minimize" onclick={() => appWindow.minimize()}>{@render icon('min')}</button>
        <button class="winbtn max" aria-label={maximized ? 'Restore' : 'Maximize'} onclick={() => appWindow.toggleMaximize()}>
          {@render icon(maximized ? 'restore' : 'max')}
        </button>
        <button class="winbtn close" aria-label="Close notepad" onclick={() => appWindow.close()}>{@render icon('close')}</button>
      </div>
    </div>
  </header>

  {#if !note}
    <div class="nonote">
      <span class="noico">{@render icon('note')}</span>
      <p class="n1">No meeting open</p>
      <p class="n2">The notepad opens when a meeting starts recording. Reopen it from a meeting note in Yap.</p>
    </div>
  {:else}
    <div class="titleblock">
      <textarea
        class="title"
        class:auto={note.titleAuto}
        rows="1"
        placeholder="New note"
        aria-label="Meeting title"
        bind:this={titleEl}
        bind:value={note.title}
        oninput={() => queueSave('title')}
        onkeydown={onTitleKeydown}
      ></textarea>
      <p class="date">{dateLine(note.createdTs)}</p>
    </div>

    <div class="tabsrow">
      <div class="tablist" role="tablist" aria-label="Meeting notepad">
        <button role="tab" class="tab" aria-selected={tab === 'thoughts'} data-tip="Your own notes, as you go" onclick={() => (tab = 'thoughts')}>
          My thoughts
        </button>
        <button role="tab" class="tab" aria-selected={tab === 'transcript'} onclick={() => (tab = 'transcript')}>
          {#if recordingThis}
            <span class="wave" aria-hidden="true"><i></i><i></i><i></i></span>
          {/if}
          Transcript
        </button>
        <button role="tab" class="tab" aria-selected={tab === 'summary'} onclick={() => (tab = 'summary')}>
          {#if !hasSummary}<span class="plus" aria-hidden="true">{@render icon('plus')}</span>{/if}
          Summary
        </button>
      </div>
    </div>

    {#key tab}
      <main class="body">
        {#if tab === 'thoughts'}
          <textarea
            class="thoughts"
            aria-label="My thoughts"
            placeholder={recordingThis
              ? 'Jot down your own notes while Yap listens. They go into the summary.'
              : 'Your own notes on this meeting.'}
            bind:value={note.content}
            oninput={() => queueSave('content')}
          ></textarea>
        {:else if tab === 'transcript'}
          {#if recordingThis || lineCount}
          <div class="timerbox">
            <span class="clockico">{@render icon('clock')}</span>
            <span class="elapsed">{recordingThis ? clock(elapsed) : clock(meetingSecs)}</span>
            <span class="grow"></span>
            <div class="tboxbtns">
              <button class="tbtn search" aria-label="Search the transcript" aria-expanded={searchOpen} onclick={toggleSearch}>
                {@render icon('search')}
              </button>
              <input
                class="tsearch"
                class:open={searchOpen}
                bind:this={searchEl}
                bind:value={query}
                placeholder="Search"
                aria-label="Search the transcript"
                tabindex={searchOpen ? 0 : -1}
                onkeydown={(e) => e.key === 'Escape' && (e.stopPropagation(), closeSearch())}
              />
              <button class="tbtn" aria-label="Copy transcript" onclick={copyTranscript} disabled={!lineCount}>
                {@render icon(transcriptCopied ? 'check' : 'copy')}
              </button>
            </div>
            {#if !hintGone}
              <div class="hintwrap">
                <div class="hint">
                  <p>Yap transcribes this on your PC as people talk: lines land about every 15 seconds, and the last few when you stop.</p>
                  <button class="hintx" aria-label="Dismiss the tip" onclick={dismissHint}>{@render icon('x')}</button>
                </div>
              </div>
            {/if}
          </div>
          {/if}
          {#if recordingThis && !liveTranscript}
            <div class="tempty">
              <p class="t1">Live transcript is off</p>
              <p class="t2">Yap is still transcribing on this PC. The transcript shows here when you stop.</p>
            </div>
          {:else if searchQuery && !items.length}
            <div class="tempty">
              <p class="t1">Nothing matches</p>
              <p class="t2">No line says “{searchQuery}”.</p>
            </div>
          {:else if !items.length}
            <div class="tempty">
              {#if recordingThis}
                <p class="t1">Yap is listening</p>
                <p class="t2">Say hello: your first line lands here in about 15 seconds. Your mic is You, the call is Them.</p>
              {:else}
                <p class="t1">No transcript yet</p>
                <p class="t2">Press Start and Yap transcribes the meeting here, on this PC.</p>
              {/if}
            </div>
          {:else}
            <div
              class="lines"
              role="log"
              aria-label="Transcript"
              bind:this={logEl}
              onscroll={() => (followLog = logEl.scrollHeight - logEl.scrollTop - logEl.clientHeight < 40)}
            >
              {#each items as item (item.key)}
                {#if item.kind === 'group'}
                  <div class="group {item.source === 'you' ? 'you' : 'them'}" class:echo={item.echo}>
                    <span class="who">{item.source === 'you' ? 'You' : 'Them'}{item.echo ? ' · from the speakers' : ''}</span>
                    <div class="bubbles">
                      {#each item.lines as line, j (line.i)}
                        <div class="row">
                          <div class="bubble {bubbleShape(j, item.lines.length)}">
                            {#each highlightParts(line.text, searchQuery) as part, k (k)}{#if part.hit}<mark>{part.text}</mark>{:else}{part.text}{/if}{/each}
                          </div>
                          <button class="linecopy" aria-label="Copy line" onclick={() => copyLine(line)}>
                            {@render icon(copiedLine === line.i ? 'check' : 'copy')}
                          </button>
                        </div>
                      {/each}
                    </div>
                  </div>
                {:else if item.kind === 'pause'}
                  <div class="divider" role="separator" aria-label="Recording paused here">
                    <span class="rule"></span>{@render icon('pause')}<span class="rule"></span>
                  </div>
                {:else}
                  <div
                    class="divider dictated"
                    role="separator"
                    aria-label="You dictated here"
                    title="Yap kept what you dictated out of the meeting's transcript and summaries"
                  >
                    <span class="rule"></span>{@render icon('mic')}<span class="dtext">You dictated here · left out of the notes</span><span class="rule"></span>
                  </div>
                {/if}
              {/each}
              {#if echoCount && !searchQuery}
                <button class="echonote" onclick={() => (showEcho = !showEcho)}>
                  {showEcho ? 'Hide' : 'Show'} {echoCount} {echoCount === 1 ? 'line' : 'lines'} your mic picked up from the speakers
                </button>
              {/if}
            </div>
          {/if}
        {:else}
          <div class="summary" role="region" aria-label="Summary">
            {#if writing}
              <div class="steps" aria-live="polite">
                <span class="stepdot" aria-hidden="true"></span>
                <span class="stepline">{stepLine(job, progress, minutes)}</span>
                <span class="stepcount">Step {job.step} of {job.steps}</span>
              </div>
            {:else if job?.state === 'needsAi'}
              <div class="aicard">
                <p><strong>Your meeting is saved.</strong> To turn it into an action plan, Yap needs an AI model: set one up in Language Models (an on-device model keeps everything on this PC), then press Generate summary.</p>
                <button class="ink" onclick={() => openSettings('cleanup')}>Open Language Models</button>
              </div>
            {:else if job?.state === 'nothing'}
              <p class="sumnote">Nothing to summarise yet: no speech was transcribed and nothing was typed.</p>
            {/if}

            {#if hasSummary && !writing}
              <!-- renderMarkdown escapes all input first (lib/markdown.js). -->
              <div class="rendered">{@html renderMarkdown(note.enhancedContent)}</div>
            {:else if !writing && job?.state !== 'needsAi'}
              <p class="sumempty">
                {@render icon('note')}
                {recordingThis ? 'Your summary is written when you stop' : 'No summary yet'}
              </p>
              {#if note.digests?.length}
                <div class="sofar">
                  <p class="sofarcap">AI notes so far</p>
                  {#each note.digests as d, i (i)}
                    <ul>
                      {#each d.keyPoints || [] as p, j (j)}<li>{p}</li>{/each}
                      {#each d.actions || [] as a, j (j)}<li class="task">☐ {a.owner}: {a.task}{a.due ? ` (due ${a.due})` : ''}</li>{/each}
                    </ul>
                  {/each}
                </div>
              {/if}
            {/if}
          </div>
        {/if}
      </main>
    {/key}

    <div class="bottom">
      <div class="fade" aria-hidden="true"></div>
      {#if !recordingThis && !askOpen && job?.state === 'error'}
        <div class="snag" role="alert">
          <span class="snagico">{@render icon('warn')}</span>
          <span class="snagtext">
            <strong>The summary didn't come through</strong>
            <span class="snagdetail" title={job.error}>{job.error}</span>
          </span>
          <button class="snagretry" aria-label="Retry" title="Try again" onclick={generate}>{@render icon('retry')}</button>
        </div>
      {:else if !recordingThis && !writing && !hasSummary && !askOpen && (note.transcript?.length || note.content?.trim())}
        <button class="gen" onclick={generate}>{@render icon('plus')}Generate summary</button>
      {/if}
      {#if recordingThis && !askOpen}
        <div class="consent popwrap">
          Let people know you're taking notes.
          <button class="learn" aria-haspopup="dialog" aria-expanded={menu === 'consent'} onclick={openConsent}>
            Learn more
          </button>
          {#if menu === 'consent'}
            <div class="pop consentpop" role="dialog" aria-label="Consent message">
              <p class="consentwhy">Let people know you're taking notes. Yap transcribes on this PC; nothing is uploaded.</p>
              <textarea
                class="consentmsg"
                rows="4"
                aria-label="Message for the meeting chat"
                bind:value={consentDraft}
                onblur={saveConsent}
              ></textarea>
              <div class="consentbtns">
                <button class="ink small" onclick={() => copyConsent(false)}>{@render icon('copy')}Copy</button>
              </div>
            </div>
          {/if}
        </div>
      {/if}
      <div class="tray" class:open={askOpen}>
        {#if askOpen}
          <div class="sheet" role="dialog" aria-label="Ask about this meeting">
            <div class="sheethead">
              <!-- The grab handle: a mouse target; the − button is the accessible one. -->
              <button class="handle" aria-hidden="true" tabindex="-1" onclick={closeAsk}><span></span></button>
              <div class="sheetbtns">
                <button class="sbtn" aria-label="New chat" data-tip="New chat" onclick={newChat}>{@render icon('newchat')}</button>
                <button class="sbtn" aria-label="Minimise chat" data-tip="Minimise" onclick={closeAsk}>{@render icon('min')}</button>
              </div>
            </div>
            <div class="thread" bind:this={threadEl} aria-live="polite">
              {#if !askThread.length && !askSetup && !askBusy}
                <p class="threadhint">Ask anything about this meeting. Yap answers from the transcript and your notes{recordingThis ? ', and What did I miss? catches you up on what was said since you last looked' : ''}.</p>
              {/if}
              {#each askThread as m (m.id)}
                {#if m.role === 'user'}
                  <p class="mq">{m.text}</p>
                {:else}
                  <div class="ma">
                    <!-- renderMarkdown escapes all input first. -->
                    <div class="mabody" class:err={m.error}>{@html renderMarkdown(m.text)}</div>
                    {#if !m.error}
                      <div class="mabtns">
                        <button class="fb" class:on={feedback[m.id]?.rating === 'up'} aria-label="Good answer" aria-pressed={feedback[m.id]?.rating === 'up'} onclick={() => rate(m, 'up')}>{@render icon('up')}</button>
                        <button class="fb" class:on={feedback[m.id]?.rating === 'down'} aria-label="Bad answer" aria-pressed={feedback[m.id]?.rating === 'down'} onclick={() => rate(m, 'down')}>{@render icon('down')}</button>
                        <button class="fb" aria-label="Copy answer" onclick={() => copyAnswer(m)}>{@render icon(copiedAnswer === m.id ? 'check' : 'copy')}</button>
                      </div>
                    {/if}
                  </div>
                {/if}
              {/each}
              {#if askBusy}
                <p class="ma busy"><span class="dots" aria-hidden="true"><i></i><i></i><i></i></span>Catching you up…</p>
              {/if}
              {#if askSetup}
                <div class="ma setup">
                  <p>Yap needs an AI model to answer. Set one up in Language Models; an on-device model keeps everything on this PC.</p>
                  <button class="ink small" onclick={() => openSettings('cleanup')}>Open Language Models</button>
                </div>
              {/if}
            </div>
          </div>
        {/if}
        <div class="barrow">
          <button
            class="rec"
            class:circle={askOpen}
            aria-label={recordingThis ? (stopping ? 'Stopping…' : 'Stop') : note.transcript?.length ? 'Resume' : 'Start'}
            disabled={(!recordingThis && meeting.recording) || stopping}
            title={!recordingThis && meeting.recording ? 'Yap is recording another meeting' : ''}
            onclick={() => (recordingThis ? stop() : resume())}
          >
            <span class="recin">
              {@render recIcon(recordingThis)}
              <span class="reclabel">{recordingThis ? (stopping ? 'Stopping…' : 'Stop') : note.transcript?.length ? 'Resume' : 'Start'}</span>
            </span>
          </button>
          <div class="ask" class:focus={askFocused}>
            <textarea
              rows="1"
              placeholder="Ask anything"
              aria-label="Ask anything"
              bind:this={askEl}
              bind:value={askInput}
              onfocus={() => {
                askFocused = true;
                openAsk();
              }}
              onblur={() => (askFocused = false)}
              oninput={fitAsk}
              onkeydown={onAskKeydown}
            ></textarea>
            {#if askInput.trim()}
              <button class="send" aria-label="Ask" onclick={sendTyped} disabled={askBusy}>{@render icon('send')}</button>
            {:else if recordingThis}
              <button class="chip" onclick={() => ask(null)} disabled={askBusy}>What did I miss?</button>
            {/if}
          </div>
        </div>
      </div>
    </div>

    {#if confirmOpen}
      <div class="scrim" role="presentation" onclick={(e) => e.target === e.currentTarget && (confirmOpen = false)}>
        <div class="dialog" role="alertdialog" aria-modal="true" aria-labelledby="np-del-title" aria-describedby="np-del-body">
          <div>
            <h2 id="np-del-title">Delete this meeting?</h2>
            <p id="np-del-body">
              Its transcript, your notes and the summary go for good.{recordingThis ? ' The recording stops first.' : ''}
            </p>
          </div>
          <div class="dbtns">
            <button class="dbtn" onclick={() => (confirmOpen = false)}>Cancel</button>
            <button class="dbtn danger" onclick={deleteNote} disabled={deleting}>{deleting ? 'Deleting…' : 'Delete'}</button>
          </div>
        </div>
      </div>
    {/if}
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
  /* A bare element selector, so every button's own class (its size and
     weight) wins over this. */
  button {
    font: inherit;
    cursor: pointer;
  }
  .pad :global(svg) {
    flex: 0 0 auto;
  }

  /* ---- header: 52px, Wispr's buttons and spacing ---- */
  .hdr {
    position: relative;
    z-index: 30;
    flex: 0 0 52px;
    display: flex;
    align-items: center;
    padding: 0 12px;
    user-select: none;
    -webkit-user-select: none;
  }
  .hdrspace {
    flex: 1;
    align-self: stretch;
  }
  .hdrright {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .iconbtn {
    position: relative;
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
    transition:
      background-color 0.15s,
      opacity 0.15s;
  }
  .iconbtn :global(svg) {
    width: 16px;
    height: 16px;
  }
  .iconbtn:hover,
  .iconbtn.open {
    background: var(--yap-paper-s2-strong);
  }
  .iconbtn.back {
    background: var(--yap-paper-s2);
    transition: background-color 0.4s var(--yap-ease-spring);
  }
  .iconbtn.back:hover {
    background: var(--yap-paper-s2-strong);
  }
  .sharegroup {
    display: flex;
    gap: 2px;
  }
  .sharebtn {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 32px;
    padding: 0 12px;
    border: none;
    border-radius: 8px 0 0 8px;
    background: var(--yap-paper-s2);
    color: var(--yap-paper-ink);
    font-size: 15px;
    font-weight: 600;
    line-height: 20px;
    transition: background-color 0.4s var(--yap-ease-spring);
  }
  .sharebtn :global(svg) {
    width: 14px;
    height: 14px;
  }
  .sharebtn:hover,
  .sharebtn.open,
  .linkbtn:hover {
    background: var(--yap-paper-s2-strong);
  }
  .linkbtn {
    position: relative;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 36px;
    height: 32px;
    padding: 0;
    border: none;
    border-radius: 0 8px 8px 0;
    background: var(--yap-paper-s2);
    color: var(--yap-paper-ink);
    transition: background-color 0.1s;
  }
  .swap {
    position: relative;
    width: 16px;
    height: 16px;
  }
  .swap > span {
    position: absolute;
    inset: 0;
    display: flex;
    transition: opacity 0.1s;
  }
  .swap :global(svg) {
    width: 16px;
    height: 16px;
  }
  .swap .b,
  .swap.on .a {
    opacity: 0;
  }
  .swap.on .b {
    opacity: 1;
  }
  .winbtns {
    display: flex;
  }
  .winbtn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 40px;
    height: 32px;
    padding: 0;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: var(--yap-paper-ink);
    transition: background-color 0.15s;
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
  .winbtn.close:hover {
    background: #c42b1c;
    color: #fff;
  }

  /* Tooltips (Wispr's: ink, 12px semibold, 15px under the button, from
     scale .98 and 4px up; a beat before they show). */
  [data-tip]::after,
  [data-tip]::before {
    position: absolute;
    left: 50%;
    z-index: 60;
    opacity: 0;
    pointer-events: none;
    transition:
      opacity 0.15s var(--yap-ease-spring),
      transform 0.15s var(--yap-ease-spring);
  }
  [data-tip]::after {
    content: attr(data-tip);
    top: calc(100% + 15px);
    padding: 6px 12px;
    border-radius: 8px;
    background: var(--yap-paper-ink);
    color: var(--yap-paper-s2);
    font-size: 12px;
    font-weight: 600;
    line-height: 20px;
    white-space: nowrap;
    transform: translateX(-50%) translateY(-4px) scale(0.98);
  }
  [data-tip]::before {
    content: '';
    top: calc(100% + 9px);
    border: 6px solid transparent;
    border-top: none;
    border-bottom-color: var(--yap-paper-ink);
    transform: translateX(-50%) translateY(-4px);
  }
  [data-tip]:hover::after,
  [data-tip]:hover::before,
  [data-tip]:focus-visible::after,
  [data-tip]:focus-visible::before {
    opacity: 1;
    transform: translateX(-50%);
    transition-delay: 0.35s;
  }
  .tipstart::after {
    left: 0;
    transform: translateY(-4px) scale(0.98);
  }
  .tipstart:hover::after,
  .tipstart:focus-visible::after {
    transform: none;
  }
  .tipstart::before {
    left: 16px;
  }

  /* ---- ⋯ menu and the Share popover ---- */
  .popwrap {
    position: relative;
  }
  .pop {
    position: absolute;
    top: calc(100% + 4px);
    right: 0;
    z-index: 50;
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 184px;
    box-sizing: border-box;
    padding: 6px;
    border: 1px solid var(--yap-paper-s2);
    border-radius: 10px;
    background: var(--yap-paper);
    box-shadow: var(--yap-shadow-popover);
    animation: popin 0.08s ease-out;
  }
  @keyframes popin {
    from {
      opacity: 0;
      translate: 0 -4px;
    }
  }
  .item {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 36px;
    box-sizing: border-box;
    padding: 8px 10px;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--yap-paper-ink);
    font-size: 15px;
    line-height: 20px;
    text-align: left;
    white-space: nowrap;
  }
  .item :global(svg) {
    width: 14px;
    height: 14px;
    color: var(--yap-paper-text-2);
  }
  .item:hover {
    background: var(--yap-paper-s2);
  }
  .item.danger,
  .item.danger :global(svg) {
    color: var(--yap-paper-danger);
  }
  .pop.share {
    width: 272px;
    padding-top: 12px;
  }
  .pophead {
    margin: 0 10px 2px;
    font-size: 15px;
    font-weight: 600;
    line-height: 20px;
  }
  .popsub {
    margin: 0 10px 8px;
    font-size: 12px;
    line-height: 17px;
    color: var(--yap-paper-date);
  }

  /* ---- title and date ---- */
  .titleblock {
    flex: 0 0 auto;
    padding: 12px 20px 0;
  }
  .title {
    display: block;
    width: 100%;
    box-sizing: border-box;
    margin: 0;
    padding: 8px 12px;
    border: none;
    background: transparent;
    color: var(--yap-paper-ink);
    font-family: var(--yap-font-display);
    font-size: 36px;
    font-weight: 400;
    line-height: 45px;
    letter-spacing: -0.04em;
    resize: none;
    overflow: hidden;
    outline: none;
  }
  .title::placeholder,
  .title.auto {
    color: var(--yap-paper-faint);
  }
  .title.auto:focus {
    color: var(--yap-paper-ink);
  }
  .date {
    margin: -8px 0 0;
    padding: 0 12px 8px;
    font-size: 12px;
    line-height: 16px;
    color: var(--yap-paper-date);
  }

  /* ---- tabs: 44px, hairline under the whole row ---- */
  .tabsrow {
    flex: 0 0 44px;
    margin-top: 4px;
    padding: 0 20px;
    border-bottom: 1px solid var(--yap-paper-s2);
  }
  .tablist {
    display: flex;
    height: 100%;
  }
  .tab {
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 12px;
    border: none;
    background: linear-gradient(var(--yap-paper-ink), var(--yap-paper-ink)) no-repeat 12px 100% / 0 2px;
    color: var(--yap-paper-text-2);
    font-size: 15px;
    font-weight: 600;
    line-height: 20px;
    transition: color 0.15s var(--yap-ease-spring);
  }
  .tab:hover {
    color: var(--yap-paper-ink);
  }
  .tab[aria-selected='true'] {
    color: var(--yap-paper-ink);
    background-size: calc(100% - 24px) 2px;
  }
  .plus {
    display: inline-flex;
  }
  .plus :global(svg) {
    width: 11px;
    height: 11px;
    stroke-width: 3.2;
  }
  /* The live waveform on the Transcript tab while recording. */
  .wave {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 2px;
    width: 12px;
    height: 16px;
  }
  .wave i {
    width: 2px;
    height: 11px;
    border-radius: 0.5px;
    background: var(--yap-live);
    animation: wavebar 0.9s ease-in-out infinite;
  }
  .wave i:nth-child(2) {
    animation-delay: 0.1s;
  }
  .wave i:nth-child(3) {
    animation-delay: -0.1s;
  }
  @keyframes wavebar {
    0%,
    100% {
      transform: scaleY(0.35);
    }
    50% {
      transform: scaleY(1);
    }
  }

  /* ---- the tab bodies ---- */
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
  .grow {
    flex: 1;
  }
  .thoughts {
    flex: 1 1 auto;
    min-height: 0;
    margin: 0 20px;
    padding: 20px 12px 180px;
    border: none;
    background: transparent;
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 15px;
    line-height: 24px;
    resize: none;
    outline: none;
    user-select: text;
  }
  .thoughts::placeholder {
    color: var(--yap-paper-faint);
  }

  /* The timer box (clock, time, search, copy) and its tip strip. */
  .timerbox {
    flex: 0 0 auto;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    column-gap: 7px;
    margin: 24px 40px 0 32px;
    padding: 10px 12px;
    border-radius: 8px;
    background: var(--yap-paper-s1);
    color: var(--yap-paper-muted);
  }
  .clockico {
    display: inline-flex;
  }
  .clockico :global(svg) {
    width: 15px;
    height: 15px;
  }
  .elapsed {
    font-size: 12px;
    font-weight: 600;
    line-height: 20px;
    letter-spacing: 0.9px;
    font-variant-numeric: tabular-nums;
  }
  .tboxbtns {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .tbtn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    padding: 0;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--yap-paper-faint);
    transition:
      background-color 0.15s,
      color 0.15s;
  }
  .tbtn :global(svg) {
    width: 16px;
    height: 16px;
  }
  .tbtn.search :global(svg) {
    width: 20px;
    height: 20px;
  }
  .tbtn:hover:not(:disabled),
  .tbtn[aria-expanded='true'] {
    background: var(--yap-paper-s2);
    color: var(--yap-paper-text-2);
  }
  .tbtn:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .tsearch {
    width: 0;
    box-sizing: border-box;
    padding: 1px 0;
    border: none;
    background: transparent;
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 15px;
    line-height: 20px;
    outline: none;
    transition:
      width 0.2s ease-out,
      padding 0.2s ease-out;
  }
  .tsearch.open {
    width: 180px;
    padding: 1px 2px;
  }
  .tsearch::placeholder {
    color: var(--yap-paper-faint);
  }
  .hintwrap {
    flex: 0 0 calc(100% + 24px);
    margin: 0 -12px -10px;
    padding-top: 7px;
    font-size: 12px;
    line-height: 20px;
  }
  .hint {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 8px 6px 12px;
    border-radius: 0 0 8px 8px;
    background: var(--yap-paper-s2);
  }
  .hint p {
    flex: 1;
    margin: 0;
  }
  .hintx {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: 0 0 24px;
    width: 24px;
    height: 24px;
    padding: 0;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--yap-paper-muted);
  }
  .hintx :global(svg) {
    width: 14px;
    height: 14px;
  }
  .hintx:hover {
    background: var(--yap-paper-s2-strong);
    color: var(--yap-paper-ink);
  }

  /* Speaker groups of chat bubbles (Wispr's grouped corners). */
  .lines {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    scrollbar-gutter: stable;
    display: flex;
    flex-direction: column;
    gap: 16px;
    margin-top: 14px;
    padding: 12px 20px 164px 36px;
  }
  /* Thin scrollbars, as on the page they sit on (8px, a soft thumb). */
  .lines::-webkit-scrollbar,
  .summary::-webkit-scrollbar,
  .thread::-webkit-scrollbar,
  .thoughts::-webkit-scrollbar {
    width: 8px;
  }
  .lines::-webkit-scrollbar-thumb,
  .summary::-webkit-scrollbar-thumb,
  .thread::-webkit-scrollbar-thumb,
  .thoughts::-webkit-scrollbar-thumb {
    border-radius: 4px;
    background: rgb(26 26 26 / 0.14);
  }
  .group {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .bubbles {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .who {
    font-size: 15px;
    font-weight: 550;
    line-height: 18px;
  }
  .group.them .who {
    color: var(--them);
  }
  .row {
    position: relative;
    padding-right: 24px;
  }
  .bubble {
    width: fit-content;
    max-width: 100%;
    box-sizing: border-box;
    padding: 8px 12px;
    background: var(--yap-paper-s1);
    font-size: 15px;
    font-weight: 500;
    line-height: 22.5px;
    overflow-wrap: anywhere;
    user-select: text;
    -webkit-user-select: text;
  }
  .bubble.first {
    border-radius: 12px 12px 12px 4px;
  }
  .bubble.mid {
    border-radius: 4px 12px 12px 4px;
  }
  .bubble.last {
    border-radius: 4px 12px 12px 12px;
  }
  .group.echo .bubble {
    opacity: 0.55;
    font-style: italic;
  }
  .bubble mark {
    border-radius: 3px;
    background: rgb(253 224 71 / 0.6);
    color: inherit;
  }
  .linecopy {
    position: absolute;
    top: 50%;
    right: 0;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    padding: 0;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--yap-paper-text-2);
    opacity: 0;
    transform: translateY(-50%);
    transition: opacity 0.12s;
  }
  .linecopy :global(svg) {
    width: 14px;
    height: 14px;
  }
  .row:hover .linecopy,
  .linecopy:focus-visible {
    opacity: 1;
  }
  .linecopy:hover {
    background: var(--yap-paper-s2);
  }
  /* Where a recording stopped (⏸), or the person dictated mid-meeting. */
  .divider {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 32px;
    box-sizing: border-box;
    padding: 10px 0;
    color: var(--yap-paper-rule);
  }
  .divider .rule {
    flex: 1;
    border-top: 1px dashed currentColor;
  }
  .divider :global(svg) {
    width: 12px;
    height: 12px;
  }
  .divider.dictated :global(svg) {
    stroke-width: 2.2;
  }
  .dtext {
    font-size: 12px;
    line-height: 16px;
    color: var(--yap-paper-date);
    white-space: nowrap;
  }
  .echonote {
    align-self: flex-start;
    padding: 0;
    border: none;
    background: none;
    color: var(--yap-paper-muted);
    font-size: 12px;
  }
  .echonote:hover {
    color: var(--yap-paper-ink);
    text-decoration: underline;
  }
  .tempty {
    margin: 72px auto 0;
    max-width: 380px;
    padding: 0 20px;
    text-align: center;
  }
  .tempty .t1 {
    margin: 0;
    font-size: 13px;
    color: var(--yap-paper-faint);
  }
  .tempty .t2 {
    margin: 6px 0 0;
    font-size: 17px;
    line-height: 24px;
    color: var(--yap-paper-date);
  }

  /* ---- Summary ---- */
  /* Rows here line up with the transcript's timer box (x 32, padded 12). */
  .summary {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    padding: 24px 40px 180px 32px;
  }
  .steps {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 12px;
  }
  .stepdot {
    flex: 0 0 auto;
    width: 5px;
    height: 5px;
    margin: 0 6.5px;
    border-radius: 50%;
    background: var(--yap-paper-ink);
    animation: breathe 1.2s ease-in-out infinite;
  }
  @keyframes breathe {
    50% {
      opacity: 0.35;
    }
  }
  .stepline {
    flex: 1;
    font-size: 13px;
  }
  .stepcount {
    font-size: 12px;
    color: var(--yap-paper-faint);
    font-variant-numeric: tabular-nums;
  }
  .aicard {
    margin-bottom: 14px;
    padding: 12px 14px;
    border-radius: 12px;
    background: var(--yap-paper-s1);
  }
  .aicard p {
    margin: 0 0 10px;
    font-size: 13px;
    line-height: 20px;
  }
  .sumnote {
    margin: 0 0 12px;
    padding: 0 12px;
    font-size: 13px;
    color: var(--yap-paper-muted);
  }
  .sumempty {
    display: flex;
    align-items: center;
    gap: 7px;
    margin: 0;
    padding: 10px 12px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--yap-paper-faint);
  }
  .sumempty :global(svg) {
    width: 15px;
    height: 15px;
  }
  .rendered {
    padding: 0 12px;
    font-size: 15px;
    line-height: 24px;
    user-select: text;
    -webkit-user-select: text;
  }
  /* renderMarkdown shifts levels: "## Action plan" is an h3, "### Alice" an
     h4. Sections in the display serif, people in the UI sans. */
  .rendered :global(h2),
  .rendered :global(h3),
  .rendered :global(h4),
  .rendered :global(h5) {
    margin: 18px 0 6px;
    line-height: 1.3;
  }
  .rendered :global(h2),
  .rendered :global(h3) {
    font-family: var(--yap-font-display);
    font-size: 24px;
    font-weight: 400;
    letter-spacing: -0.02em;
  }
  .rendered :global(h4),
  .rendered :global(h5) {
    margin-top: 12px;
    font-size: 15px;
    font-weight: 600;
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
  .sofar {
    margin-top: 8px;
    padding: 0 12px;
    font-size: 14px;
    line-height: 21px;
  }
  .sofarcap {
    margin: 0 0 4px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--yap-paper-faint);
  }
  .sofar ul {
    margin: 0 0 8px;
    padding-left: 18px;
  }
  .sofar li.task {
    list-style: none;
    margin-left: -14px;
  }
  .ink {
    height: 32px;
    padding: 0 14px;
    border: none;
    border-radius: 9999px;
    background: var(--yap-paper-ink);
    color: var(--yap-paper);
    font-size: 13px;
    font-weight: 600;
  }
  .ink.small {
    height: 28px;
    font-size: 12.5px;
  }

  /* ---- the bottom: fade, consent, Generate summary, the bar ---- */
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
  .gen {
    position: absolute;
    left: 50%;
    bottom: 98px;
    display: inline-flex;
    align-items: center;
    gap: 8px;
    height: 40px;
    box-sizing: border-box;
    padding: 10px 24px;
    border: none;
    border-radius: 9999px;
    background: var(--yap-paper-ink);
    color: var(--yap-paper);
    font-size: 15px;
    font-weight: 550;
    line-height: 20px;
    white-space: nowrap;
    transform: translateX(-50%);
    pointer-events: auto;
    transition: background-color 0.4s var(--yap-ease-spring);
  }
  .gen :global(svg) {
    width: 15px;
    height: 15px;
    stroke-width: 2.4;
  }
  .gen:hover {
    background: rgb(56 56 56);
  }
  /* The summary's error, where Generate summary would be, with a Retry. */
  .snag {
    position: absolute;
    left: 12px;
    right: 12px;
    bottom: 98px;
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 40px;
    box-sizing: border-box;
    padding: 0 4px 0 12px;
    color: var(--yap-paper-snag);
    font-size: 13px;
    line-height: 18px;
    pointer-events: auto;
  }
  .snagico {
    display: inline-flex;
  }
  .snagico :global(svg) {
    width: 15px;
    height: 15px;
  }
  .snagtext {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .snagtext strong {
    font-weight: 500;
  }
  .snagdetail {
    font-size: 12px;
    color: var(--yap-paper-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .snagretry {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: 0 0 28px;
    width: 28px;
    height: 28px;
    padding: 0;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--yap-paper-snag);
    transition: background-color 0.15s;
  }
  .snagretry :global(svg) {
    width: 16px;
    height: 16px;
  }
  .snagretry:hover {
    background: var(--yap-paper-s2);
  }
  .consent {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 88px;
    margin: 0;
    text-align: center;
    font-size: 12px;
    line-height: 18px;
    color: var(--yap-paper-date);
    pointer-events: auto;
  }
  .learn {
    padding: 0;
    border: none;
    background: none;
    color: inherit;
    font-size: inherit;
    text-decoration: underline;
    text-underline-offset: 2px;
  }
  .learn:hover,
  .learn[aria-expanded='true'] {
    color: var(--yap-paper-ink);
  }
  /* "Learn more": the consent message, in the ⋯ menu's popover style. */
  .pop.consentpop {
    top: auto;
    right: auto;
    bottom: calc(100% + 8px);
    left: 50%;
    width: min(400px, calc(100vw - 72px));
    gap: 10px;
    padding: 12px;
    text-align: left;
    transform: translateX(-50%);
  }
  .consentwhy {
    margin: 0;
    font-size: 13px;
    line-height: 19px;
    color: var(--yap-paper-text-2);
  }
  .consentmsg {
    display: block;
    width: 100%;
    box-sizing: border-box;
    padding: 8px 10px;
    border: 1px solid var(--yap-paper-s2);
    border-radius: 8px;
    background: #fff;
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 14px;
    line-height: 21px;
    resize: vertical;
    outline: none;
    user-select: text;
    -webkit-user-select: text;
    transition:
      border-color 0.15s,
      box-shadow 0.15s;
  }
  .consentmsg:focus {
    border-color: var(--yap-paper-text-2);
    box-shadow: 0 0 0 3px var(--yap-paper-ring);
  }
  .consentbtns {
    display: flex;
    justify-content: flex-end;
  }
  .consentbtns .ink {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .consentbtns .ink :global(svg) {
    width: 13px;
    height: 13px;
  }
  .tray {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 16px;
    height: 70px;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    border: 1px solid transparent;
    border-radius: 32px;
    background: var(--yap-paper);
    pointer-events: auto;
    transition:
      height 0.25s ease-out,
      box-shadow 0.2s,
      border-color 0.2s;
  }
  .tray.open {
    height: clamp(300px, 45vh, 720px);
    box-shadow: var(--yap-shadow-sheet);
  }
  .sheet {
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .sheethead {
    flex: 0 0 55px;
    box-sizing: border-box;
    padding: 0 12px 8px;
    border-bottom: 1px solid var(--yap-paper-s2);
  }
  .handle {
    display: block;
    width: 100%;
    height: 14px;
    padding: 5px 0 0;
    border: none;
    background: none;
  }
  .handle span {
    display: block;
    width: 40px;
    height: 4px;
    margin: 0 auto;
    border-radius: 999px;
    background: var(--yap-paper-s2);
    transition: background-color 0.1s;
  }
  .handle:hover span {
    background: var(--yap-paper-s2-strong);
  }
  .sheetbtns {
    display: flex;
    justify-content: flex-end;
    gap: 2px;
    margin-top: 2px;
  }
  .sbtn {
    position: relative;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    padding: 0 6px;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--yap-paper-muted);
    transition:
      background 0.15s,
      color 0.15s;
  }
  .sbtn :global(svg) {
    width: 16px;
    height: 16px;
  }
  .sbtn:hover {
    background: var(--yap-paper-s2);
    color: var(--yap-paper-ink);
  }
  .sbtn[data-tip]::after {
    left: auto;
    right: 0;
    transform: translateY(-4px) scale(0.98);
  }
  .sbtn[data-tip]:hover::after {
    transform: none;
  }
  .thread {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px 8px 12px 16px;
    font-size: 15px;
  }
  .threadhint {
    margin: 8px 8px 0 0;
    font-size: 13px;
    line-height: 20px;
    color: var(--yap-paper-date);
  }
  .mq {
    align-self: flex-end;
    max-width: 80%;
    margin: 0;
    padding: 8px 12px;
    border-radius: 18px 18px 4px 18px;
    background: var(--yap-paper-s2);
    line-height: 21px;
    overflow-wrap: anywhere;
    user-select: text;
    -webkit-user-select: text;
    animation: bubblein 0.1s ease-out both;
  }
  @keyframes bubblein {
    from {
      opacity: 0;
      transform: translateY(4px) scale(0.98);
    }
  }
  .ma {
    margin: 14px 0 0;
    padding-right: 8px;
  }
  .mabody {
    line-height: 22.5px;
    overflow-wrap: anywhere;
    user-select: text;
    -webkit-user-select: text;
  }
  .mabody.err {
    color: var(--yap-paper-danger);
  }
  .mabody :global(p) {
    margin: 0 0 6px;
  }
  .mabody :global(ul),
  .mabody :global(ol) {
    margin: 0 0 6px;
    padding-left: 20px;
  }
  .mabtns {
    display: flex;
    gap: 4px;
    margin-top: 4px;
  }
  .fb {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    padding: 0;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--yap-paper-text-2);
    transition: background 0.15s;
  }
  .fb :global(svg) {
    width: 16px;
    height: 16px;
  }
  .fb:hover {
    background: var(--yap-paper-s2);
  }
  .fb.on {
    background: var(--yap-paper-s2);
    color: var(--yap-paper-ink);
  }
  .ma.busy {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    color: var(--yap-paper-muted);
    font-size: 14px;
  }
  .dots {
    display: inline-flex;
    gap: 3px;
  }
  .dots i {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: currentColor;
    animation: breathe 1s ease-in-out infinite;
  }
  .dots i:nth-child(2) {
    animation-delay: 0.15s;
  }
  .dots i:nth-child(3) {
    animation-delay: 0.3s;
  }
  .ma.setup p {
    margin: 0 0 8px;
    font-size: 14px;
    line-height: 21px;
  }
  .barrow {
    flex: 0 0 auto;
    display: flex;
    align-items: flex-end;
    gap: 8px;
    margin-top: auto;
    padding: 10px;
  }
  .rec {
    flex: 0 0 auto;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    height: 48px;
    box-sizing: border-box;
    padding: 15px;
    border: 1px solid var(--yap-paper-s2);
    border-radius: 9999px;
    background: #fff;
    color: var(--yap-paper-ink);
    font-size: 15px;
    font-weight: 600;
    transition: all 0.1s linear;
  }
  .rec:hover:not(:disabled) {
    border-color: var(--yap-paper-s2-strong);
    background: rgb(250 250 248);
  }
  .rec:disabled {
    opacity: 0.55;
    cursor: default;
  }
  .recin {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    transition: gap 0.1s linear;
  }
  .recin :global(svg) {
    width: 16px;
    height: 16px;
    color: var(--yap-live-strong);
  }
  .reclabel {
    line-height: 16px;
    white-space: nowrap;
  }
  .rec.circle {
    width: 48px;
    border-radius: 50%;
  }
  .rec.circle .recin {
    gap: 0;
  }
  .rec.circle .reclabel {
    display: none;
  }
  .ask {
    flex: 1 1 auto;
    min-width: 0;
    min-height: 48px;
    box-sizing: border-box;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 0 10px 0 16px;
    border: 1px solid var(--yap-paper-s2);
    border-radius: 25px;
    background: #fff;
    transition:
      border-color 0.15s,
      box-shadow 0.15s;
  }
  .ask.focus {
    border-color: var(--yap-paper-text-2);
    box-shadow: 0 0 0 3px var(--yap-paper-ring);
  }
  .ask textarea {
    flex: 1 1 auto;
    min-width: 0;
    height: 22px;
    max-height: 110px;
    margin: 12px 0;
    padding: 0;
    border: none;
    background: transparent;
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 15px;
    line-height: 22px;
    resize: none;
    outline: none;
  }
  .ask textarea::placeholder {
    color: var(--yap-paper-faint);
  }
  .chip {
    flex: 0 0 auto;
    height: 28px;
    padding: 0 10px;
    border: none;
    border-radius: 9999px;
    background: var(--yap-paper-s2);
    color: var(--yap-paper-ink);
    font-size: 13px;
    font-weight: 600;
    white-space: nowrap;
    transition: background-color 0.15s;
  }
  .chip:hover:not(:disabled) {
    background: var(--yap-paper-s2-strong);
  }
  .chip:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .send {
    flex: 0 0 auto;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 30px;
    padding: 0;
    border: none;
    border-radius: 50%;
    background: var(--yap-paper-ink);
    color: var(--yap-paper);
  }
  .send :global(svg) {
    width: 15px;
    height: 15px;
    stroke-width: 2.4;
  }
  .send:disabled {
    opacity: 0.4;
    cursor: default;
  }

  /* ---- the confirm dialog (Wispr's: a scrim, a card scaling in) ---- */
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 80;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(26, 26, 26, 0.3);
    animation: bodyin 0.15s var(--yap-ease-spring);
  }
  .dialog {
    display: flex;
    flex-direction: column;
    gap: 32px;
    width: min(360px, calc(100vw - 48px));
    box-sizing: border-box;
    padding: 24px;
    border: 1px solid var(--yap-paper-s2);
    border-radius: 12px;
    background: var(--yap-paper);
    box-shadow: var(--yap-shadow-dialog);
    animation: scalein 0.15s var(--yap-ease-spring) both;
  }
  @keyframes scalein {
    from {
      opacity: 0;
      transform: scale(0.9);
    }
  }
  .dialog h2 {
    margin: 0 0 8px;
    font-size: 17px;
    font-weight: 600;
    line-height: 24px;
  }
  .dialog p {
    margin: 0;
    font-size: 14px;
    line-height: 21px;
    color: var(--yap-paper-text-2);
  }
  .dbtns {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  .dbtn {
    height: 36px;
    padding: 0 16px;
    border: 1px solid var(--yap-paper-s2);
    border-radius: 9999px;
    background: #fff;
    color: var(--yap-paper-ink);
    font-size: 14px;
    font-weight: 600;
  }
  .dbtn:hover:not(:disabled) {
    border-color: var(--yap-paper-s2-strong);
  }
  .dbtn.danger {
    border-color: transparent;
    background: rgb(214 69 69);
    color: #fff;
  }
  .dbtn.danger:hover:not(:disabled) {
    background: rgb(196 52 52);
  }
  .dbtn:disabled {
    opacity: 0.6;
    cursor: default;
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
