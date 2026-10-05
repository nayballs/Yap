<script>
  // The meeting notepad (notepad.rs) — Yap's meeting UI while a call runs,
  // after Wispr Flow's Notetaker notepad: a slim window docked to the right
  // edge of the screen beside the call. Three tabs over one meeting note:
  //   - My thoughts: the note's own content, auto-saved and kept in sync with
  //     the Notes view both ways (`yap-note-changed`, `origin` = window label);
  //   - Transcript: live You/Them lines, following the newest one, tidied into
  //     a paragraph per speaker turn once the meeting ends; echo lines hidden
  //     as in the Notes view;
  //   - Summary: the action plan Rust writes when the meeting ends
  //     (meeting_end.rs, "Step 2 of 3"), with a Retry when it fails.
  // The footer stops the meeting (Stop = "End meeting & summarise") and asks
  // "What did I miss?" (meeting_assist.rs: what was said since you last
  // looked — the transcript on screen and scrolled to its newest line, or the
  // last answer); after the meeting it offers Resume and Generate summary.
  // Like every Yap window with focusable UI, it catches the dictation hotkey
  // in-page (a focused WebView2 window never reaches the global hook).
  import { invoke } from '@tauri-apps/api/core';
  import { emit, listen } from '@tauri-apps/api/event';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { onMount, tick } from 'svelte';
  import yapIcon from '../assets/yap-logo.svg';
  import ToastHost from './ui/ToastHost.svelte';
  import { toast } from './ui/toast.svelte.js';
  import { renderMarkdown, markdownToText } from './markdown.js';
  import { hotkeyMatchesKeydown, hotkeyMatchesKeyup } from './hotkeys.js';
  import {
    summaries,
    initMeetingSummary,
    loadSummary,
    summarise,
    stepLine,
    askStartedByMistake,
  } from './meetingSummary.svelte.js';

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

  const recordingThis = $derived(!!note && meeting.recording && meeting.noteId === note.id);
  const job = $derived(note ? summaries.byNote[note.id] : null);
  const progress = $derived(note ? summaries.progress[note.id] : null);
  const writing = $derived(job?.state === 'running');

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
        missOpen = false;
        missThread = [];
        missSetup = null;
        showEcho = false;
        followLog = true;
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
      note.digests = n.digests;
      note.enhancedContent = n.enhancedContent;
      note.participants = n.participants;
    } catch {
      note = null; // gone (discarded, deleted)
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
  function clock(secs) {
    const s = Math.max(0, Math.round(secs));
    const h = Math.floor(s / 3600);
    const m = Math.floor((s % 3600) / 60);
    const sec = String(s % 60).padStart(2, '0');
    return h ? `${h}:${String(m).padStart(2, '0')}:${sec}` : `${m}:${sec}`;
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
      // The last chunk is in: the transcript as stored (and tidied).
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
  const echoCount = $derived((note?.transcript || []).filter((s) => s.echo).length);
  // Consecutive lines of one speaker are one turn (one label).
  const turns = $derived.by(() => {
    const out = [];
    for (const s of note?.transcript || []) {
      if (s.echo && !showEcho) continue;
      const last = out[out.length - 1];
      if (last && last.source === s.source && last.echo === !!s.echo) last.lines.push(s.text);
      else out.push({ source: s.source, echo: !!s.echo, lines: [s.text] });
    }
    return out;
  });
  const lineCount = $derived((note?.transcript || []).length);
  const meetingSecs = $derived.by(() => {
    const t = note?.transcript || [];
    return t.length > 1 ? t[t.length - 1].ts - t[0].ts : 0;
  });
  const minutes = $derived(note?.transcript?.length ? Math.round(meetingSecs / 60) : null);

  // The hint about the live transcript, dismissible for good.
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
    void turns.length;
    void lineCount;
    if (logEl && followLog) logEl.scrollTop = logEl.scrollHeight;
  });

  // ---- "What did I miss?" ----

  // What the person has seen, per note (transcript segments): everything,
  // whenever the transcript is on screen and scrolled to its newest line.
  const seen = new Map();
  let windowShown = $state(true);
  /** The Transcript tab is up, showing its lines, at the newest one. */
  function transcriptShown() {
    return tab === 'transcript' && followLog && (liveTranscript || !recordingThis);
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

  let missOpen = $state(false);
  let missThread = $state([]); // { role: 'user' | 'assistant', text, error? }
  let missBusy = $state(false);
  let missSetup = $state(null); // the "set up an AI model" reason
  let missInput = $state('');
  let missEl = $state(null);

  async function askMiss(question = null) {
    if (!note || missBusy) return;
    const id = note.id;
    const q = question?.trim() || null;
    // Asking with the transcript on screen, at its newest line: that's seen.
    if (!q && transcriptShown()) {
      seen.set(id, Math.max(seen.get(id) ?? 0, lineCount));
    }
    missOpen = true;
    missSetup = null;
    missThread = [...missThread, { role: 'user', text: q ?? 'What did I miss?' }];
    missBusy = true;
    scrollMiss();
    try {
      const r = await invoke('meeting_catch_up', { noteId: id, since: seen.get(id) ?? 0, question: q });
      if (note?.id !== id) return;
      missThread = [...missThread, { role: 'assistant', text: r.answer }];
      if (!q) seen.set(id, Math.max(seen.get(id) ?? 0, r.upto));
    } catch (e) {
      const msg = String(e);
      if (/No AI model configured|API key/i.test(msg)) missSetup = msg;
      else missThread = [...missThread, { role: 'assistant', text: `I couldn't answer that: ${msg}`, error: true }];
    } finally {
      missBusy = false;
      scrollMiss();
    }
  }
  function askFollowUp(e) {
    e.preventDefault();
    const q = missInput.trim();
    if (!q) return;
    missInput = '';
    askMiss(q);
  }
  async function scrollMiss() {
    await tick();
    if (missEl) missEl.scrollTop = missEl.scrollHeight;
  }

  // ---- elsewhere in Yap ----

  /** Show Yap's main window at a Settings section ('cleanup' = Language Models). */
  async function openSettings(section) {
    await invoke('open_settings').catch(() => {});
    emit('yap-open-settings', section).catch(() => {});
  }
  /** The note in the main window's Notes view. */
  async function openInYap() {
    if (!note) return;
    flushSave();
    await invoke('open_settings').catch(() => {});
    emit('yap-meeting-open-note', { noteId: note.id, stop: false }).catch(() => {});
  }

  let copied = $state(null);
  async function copySummary(asText) {
    let text = note?.enhancedContent;
    if (!text) return;
    if (asText) text = markdownToText(text);
    try {
      await navigator.clipboard.writeText(text);
      copied = asText ? 'text' : 'markdown';
      setTimeout(() => (copied = null), 1500);
    } catch {
      /* clipboard unavailable */
    }
  }

  function dateLine(ts) {
    const d = new Date((ts || 0) * 1000);
    const day = d.toLocaleDateString(undefined, { weekday: 'short', day: 'numeric', month: 'short' });
    const time = d.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' });
    return `${day} · ${time}`;
  }

  // ---- the in-page hotkey fallback (the WebView2-focus gotcha) ----

  async function refreshConfig() {
    try {
      cfg = await invoke('get_config');
      liveTranscript = cfg.meetingLiveTranscript !== false;
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
    on('yap-meeting-summary', onSummary);
    on('yap-note-changed', onNoteChanged);
    on('yap-note-deleted', (p) => p?.id === note?.id && (note = null));
    on('yap-notes-changed', () => note && reload());
    on('yap-meeting-warning', (msg) =>
      toast({ title: 'Meeting recording', description: String(msg), chip: 'Tip' })
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

<div class="pad">
  <header class="titlebar" data-tauri-drag-region>
    <div class="brand" data-tauri-drag-region>
      <img class="brandlogo" data-tauri-drag-region src={yapIcon} alt="" aria-hidden="true" />
      <span class="brandname" data-tauri-drag-region>Yap</span>
      <span class="brandsub" data-tauri-drag-region>Meeting notes</span>
    </div>
    <div class="winbtns">
      <button class="winbtn" title="Open in Yap" aria-label="Open in Yap" onclick={openInYap} disabled={!note}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M14 4h6v6" /><path d="M20 4l-9 9" /><path d="M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5" /></svg>
      </button>
      <button class="winbtn" title="Minimize" aria-label="Minimize" onclick={() => appWindow.minimize()}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" aria-hidden="true"><path d="M5 12h14" /></svg>
      </button>
      <button class="winbtn close" title="Close" aria-label="Close notepad" onclick={() => appWindow.close()}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18" /></svg>
      </button>
    </div>
  </header>

  {#if !note}
    <div class="nonote">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" /><path d="M14 2v6h6M9 13h6M9 17h4" /></svg>
      <p class="n1">No meeting open</p>
      <p class="n2">The notepad opens when a meeting starts recording. Reopen it from a meeting note in Yap.</p>
    </div>
  {:else}
    <div class="head">
      <input
        class="title"
        class:auto={note.titleAuto}
        placeholder="Untitled meeting"
        aria-label="Meeting title"
        bind:value={note.title}
        oninput={() => queueSave('title')}
      />
      <p class="meta">
        {dateLine(note.createdTs)}
        {#if recordingThis}
          · <span class="live"><span class="recdot"></span>{stopping ? 'Finishing up…' : `Recording ${clock(elapsed)}`}</span>
        {:else if minutes != null}
          · {minutes < 1 ? 'under a minute' : `${minutes} min`}
        {/if}
      </p>
    </div>

    <div class="tabs" role="tablist" aria-label="Meeting notepad">
      <button role="tab" class="tab" aria-selected={tab === 'thoughts'} onclick={() => (tab = 'thoughts')}>
        My thoughts
      </button>
      <button role="tab" class="tab" aria-selected={tab === 'transcript'} onclick={() => (tab = 'transcript')}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" aria-hidden="true"><path d="M4 10v4M8 7v10M12 4v16M16 8v8M20 11v2" /></svg>
        Transcript
      </button>
      <button role="tab" class="tab" aria-selected={tab === 'summary'} onclick={() => (tab = 'summary')}>
        {#if writing}
          <span class="tabspin" aria-hidden="true"></span>
        {:else}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3l1.8 5.2L19 10l-5.2 1.8L12 17l-1.8-5.2L5 10l5.2-1.8z" /><path d="M19 17l.7 2 2 .7-2 .7-.7 2-.7-2-2-.7 2-.7z" /></svg>
        {/if}
        Summary
      </button>
    </div>

    <section class="body" class:flush={tab === 'thoughts'}>
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
        <div class="trow">
          {#if recordingThis}
            <span class="elapsed rec"><span class="recdot"></span>{clock(elapsed)}</span>
          {:else}
            <span class="elapsed">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M4 10v4M8 7v10M12 4v16M16 8v8M20 11v2" /></svg>
              {lineCount ? clock(meetingSecs) : '0:00'}
            </span>
          {/if}
        </div>
        {#if recordingThis && !liveTranscript}
          <div class="tempty">
            <p class="t1">Live transcript is off</p>
            <p class="t2">Yap is still transcribing on this PC. The transcript shows here when you stop.</p>
          </div>
        {:else}
        {#if !hintGone}
          <div class="hint">
            <span>Lines arrive about every 15 seconds as Yap transcribes on this PC. When you stop, it fills in the last few seconds and tidies the transcript into one paragraph per speaker.</span>
            <button class="hintx" aria-label="Dismiss the tip" onclick={dismissHint}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18" /></svg>
            </button>
          </div>
        {/if}
        {#if turns.length === 0}
          <div class="tempty">
            {#if recordingThis}
              <p class="t1">Yap is listening</p>
              <p class="t2">Say hello: your first line lands here in about 15 seconds. Your mic is You, the call is Them.</p>
            {:else}
              <p class="t1">No transcript yet</p>
              <p class="t2">Press Record and Yap transcribes the meeting here, on this PC.</p>
            {/if}
          </div>
        {:else}
          <div
            class="log"
            class:tidy={!recordingThis}
            role="log"
            aria-label="Transcript"
            bind:this={logEl}
            onscroll={() => (followLog = logEl.scrollHeight - logEl.scrollTop - logEl.clientHeight < 40)}
          >
            {#each turns as turn, i (i)}
              <div class="turn {turn.source === 'you' ? 'you' : 'them'}" class:echo={turn.echo}>
                <span class="who">{turn.source === 'you' ? 'You' : 'Them'}{turn.echo ? ' · from the speakers' : ''}</span>
                {#if recordingThis}
                  {#each turn.lines as line, j (j)}<p>{line}</p>{/each}
                {:else}
                  <p>{turn.lines.join(' ')}</p>
                {/if}
              </div>
            {/each}
          </div>
        {/if}
        {#if echoCount}
          <button class="echonote" onclick={() => (showEcho = !showEcho)}>
            {showEcho ? 'Hide' : 'Show'} {echoCount} {echoCount === 1 ? 'line' : 'lines'} your mic picked up from the speakers
          </button>
        {/if}
        {/if}
      {:else}
        <div class="summary" role="region" aria-label="Summary">
          {#if writing}
            <div class="steps" aria-live="polite">
              <span class="stepdot" aria-hidden="true"></span>
              <span class="stepline">{stepLine(job, progress, minutes)}</span>
              <span class="stepcount">Step {job.step} of {job.steps}</span>
            </div>
          {:else if job?.state === 'error'}
            <div class="sumerr" role="alert">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 9v4" /><path d="M12 17h.01" /><path d="M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z" /></svg>
              <span class="errtext">
                <strong>The summary didn't come through</strong>
                <span class="errdetail">{job.error}</span>
              </span>
              <button class="retry" onclick={generate}>
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M20 12a8 8 0 1 1-2.3-5.7" /><path d="M20 4v5h-5" /></svg>
                Retry
              </button>
            </div>
          {:else if job?.state === 'needsAi'}
            <div class="aicard">
              <p><strong>Your meeting is saved.</strong> To turn it into an action plan, Yap needs an AI model: set one up in Language Models (an on-device model keeps everything on this PC), then press Generate summary.</p>
              <button class="ink" onclick={() => openSettings('cleanup')}>Open Language Models</button>
            </div>
          {:else if job?.state === 'nothing'}
            <p class="sumnote">Nothing to summarise yet: no speech was transcribed and nothing was typed.</p>
          {/if}

          {#if note.enhancedContent && !writing}
            <div class="sumhead">
              <span class="sumcap">AI summary</span>
              <span class="grow"></span>
              <button class="copy" onclick={() => copySummary(false)}>{copied === 'markdown' ? 'Copied!' : 'Copy markdown'}</button>
              <button class="copy" onclick={() => copySummary(true)}>{copied === 'text' ? 'Copied!' : 'Copy text'}</button>
            </div>
            <!-- renderMarkdown escapes all input first (lib/markdown.js). -->
            <div class="rendered">{@html renderMarkdown(note.enhancedContent)}</div>
          {:else if !writing && job?.state !== 'error' && job?.state !== 'needsAi'}
            <p class="sumempty">{recordingThis ? 'Your summary is written when you stop' : 'No summary yet'}</p>
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
    </section>

    {#if missOpen}
      <div class="miss" role="region" aria-label="What did I miss?">
        <div class="misshead">
          <span>Catch up</span>
          <button class="missx" aria-label="Close catch-up" onclick={() => (missOpen = false)}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18" /></svg>
          </button>
        </div>
        <div class="missthread" bind:this={missEl}>
          {#each missThread as m, i (i)}
            {#if m.role === 'user'}
              <p class="mq">{m.text}</p>
            {:else}
              <!-- renderMarkdown escapes all input first. -->
              <div class="ma" class:err={m.error}>{@html renderMarkdown(m.text)}</div>
            {/if}
          {/each}
          {#if missBusy}
            <p class="ma busy"><span class="tabspin" aria-hidden="true"></span> Catching you up…</p>
          {/if}
          {#if missSetup}
            <div class="ma setup">
              <p>Yap needs an AI model to catch you up. Set one up in Language Models; an on-device model keeps everything on this PC.</p>
              <button class="ink small" onclick={() => openSettings('cleanup')}>Open Language Models</button>
            </div>
          {/if}
        </div>
        <form class="missask" onsubmit={askFollowUp}>
          <input bind:value={missInput} placeholder="Ask about this meeting…" aria-label="Ask about this meeting" disabled={missBusy} />
          <button type="submit" class="send" aria-label="Ask" disabled={missBusy || !missInput.trim()}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 19V5M5 12l7-7 7 7" /></svg>
          </button>
        </form>
      </div>
    {/if}

    <footer class="foot">
      {#if recordingThis}
        <p class="consent">Always get consent when transcribing others.</p>
      {/if}
      <div class="footrow">
        {#if recordingThis}
          <button class="pill stop" onclick={stop} disabled={stopping}>
            <span class="sq" aria-hidden="true"></span>
            {stopping ? 'Stopping…' : 'Stop'}
          </button>
          <span class="grow"></span>
          <button class="missbtn" onclick={() => askMiss()} disabled={missBusy}>What did I miss?</button>
        {:else}
          <button
            class="pill resume"
            onclick={resume}
            disabled={meeting.recording}
            title={meeting.recording ? 'Yap is recording another meeting' : ''}
          >
            <span class="rdot" aria-hidden="true"></span>
            {note.transcript?.length ? 'Resume' : 'Record'}
          </button>
          <span class="grow"></span>
          {#if (note.transcript?.length || note.content?.trim()) && !writing && (!note.enhancedContent || job?.state === 'error')}
            <button class="pill gen" onclick={generate}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
              Generate summary
            </button>
          {/if}
        {/if}
      </div>
    </footer>
  {/if}

  <ToastHost />
</div>

<style>
  .pad {
    /* Toasts sit above the footer in this narrow window. */
    --yap-toast-bottom: 84px;
    --yap-toast-right: 16px;
    --them: #2e6a86;
    display: flex;
    flex-direction: column;
    height: 100vh;
    background: var(--yap-s1);
    color: var(--yap-fg);
    font-size: 13.5px;
    border-left: 1px solid var(--yap-border-subtle);
    box-sizing: border-box;
    overflow: hidden;
  }

  /* ---- custom chrome (undecorated, like the main window) ---- */
  .titlebar {
    flex: 0 0 40px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    user-select: none;
    -webkit-user-select: none;
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 8px;
    padding-left: 16px;
    min-width: 0;
  }
  .brandlogo {
    width: 20px;
    height: 20px;
    border-radius: 6px;
    pointer-events: none;
  }
  .brandname {
    font-size: 13.5px;
    font-weight: 700;
  }
  .brandsub {
    font-size: 12px;
    color: var(--yap-muted-70);
    white-space: nowrap;
  }
  .winbtns {
    display: flex;
    align-self: stretch;
  }
  .winbtn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 42px;
    height: 100%;
    border: none;
    background: none;
    color: var(--yap-fg);
    cursor: default;
    transition: background var(--yap-dur) ease;
  }
  .winbtn svg {
    width: 16px;
    height: 16px;
  }
  .winbtn:hover:not(:disabled) {
    background: var(--yap-raised);
  }
  .winbtn:disabled {
    opacity: 0.35;
  }
  .winbtn.close:hover {
    background: #c42b1c;
    color: #fff;
  }

  .nonote {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding: 0 40px 60px;
    text-align: center;
    color: var(--yap-muted);
  }
  .nonote svg {
    width: 32px;
    height: 32px;
    color: var(--yap-muted-55);
  }
  .nonote .n1 {
    margin: 4px 0 0;
    font-family: var(--yap-font-display);
    font-size: 22px;
    color: var(--yap-fg);
  }
  .nonote .n2 {
    margin: 0;
    font-size: 12.5px;
    line-height: 1.6;
  }

  /* ---- title + date ---- */
  .head {
    padding: 10px 26px 4px;
  }
  .title {
    width: 100%;
    box-sizing: border-box;
    padding: 0;
    border: none;
    background: none;
    color: var(--yap-fg);
    font-family: var(--yap-font-display);
    font-size: 28px;
    font-weight: 500;
    letter-spacing: -0.01em;
    line-height: 1.25;
    text-overflow: ellipsis;
  }
  .title:focus {
    outline: none;
  }
  .title::placeholder,
  .title.auto {
    color: var(--yap-muted-55);
  }
  .title.auto:focus {
    color: var(--yap-fg);
  }
  .meta {
    margin: 3px 0 0;
    font-size: 12px;
    color: var(--yap-muted-70);
    font-variant-numeric: tabular-nums;
  }
  .live {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    color: var(--yap-danger);
  }
  .recdot {
    display: inline-block;
    width: 7px;
    height: 7px;
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

  /* ---- tabs (underlined, Wispr-style) ---- */
  .tabs {
    display: flex;
    gap: 22px;
    padding: 12px 26px 0;
    border-bottom: 1px solid var(--yap-border-subtle);
  }
  .tab {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 0 1px 9px;
    border: none;
    border-bottom: 2px solid transparent;
    margin-bottom: -1px;
    background: none;
    color: var(--yap-muted);
    font: inherit;
    font-size: 13px;
    font-weight: 550;
    cursor: pointer;
    transition: color var(--yap-dur) ease;
  }
  .tab svg {
    width: 12px;
    height: 12px;
  }
  .tab:hover {
    color: var(--yap-fg);
  }
  .tab[aria-selected='true'] {
    color: var(--yap-fg);
    font-weight: 650;
    border-bottom-color: var(--yap-ink);
  }
  .tabspin {
    display: inline-block;
    width: 10px;
    height: 10px;
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

  /* ---- tab bodies ---- */
  .body {
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
    padding: 16px 26px 8px;
    overflow-y: auto;
  }
  .body.flush {
    overflow: hidden;
  }
  .thoughts {
    flex: 1 1 auto;
    min-height: 0;
    resize: none;
    border: none;
    background: transparent;
    color: var(--yap-fg);
    font: inherit;
    font-size: 14px;
    line-height: 1.7;
    padding: 0;
  }
  .thoughts:focus {
    outline: none;
  }
  .thoughts::placeholder {
    color: var(--yap-muted-55);
  }

  .trow {
    display: flex;
    align-items: center;
    margin-bottom: 10px;
  }
  .elapsed {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--yap-muted-70);
    font-variant-numeric: tabular-nums;
  }
  .elapsed svg {
    width: 13px;
    height: 13px;
  }
  .elapsed.rec {
    color: var(--yap-danger);
  }
  .hint {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    margin-bottom: 14px;
    padding: 9px 10px 9px 12px;
    border-radius: var(--yap-r);
    background: var(--yap-s3);
    color: var(--yap-muted);
    font-size: 11.5px;
    line-height: 1.55;
  }
  .hintx {
    flex: 0 0 auto;
    display: inline-flex;
    width: 20px;
    height: 20px;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: var(--yap-r-sm);
    background: none;
    color: var(--yap-muted-55);
    cursor: pointer;
  }
  .hintx:hover {
    background: var(--yap-raised);
    color: var(--yap-fg);
  }
  .hintx svg {
    width: 11px;
    height: 11px;
  }
  .tempty {
    margin: 48px auto 0;
    max-width: 300px;
    text-align: center;
  }
  .tempty .t1 {
    margin: 0;
    font-family: var(--yap-font-display);
    font-size: 21px;
    color: var(--yap-fg-62);
  }
  .tempty .t2 {
    margin: 6px 0 0;
    font-size: 12.5px;
    line-height: 1.6;
    color: var(--yap-muted-70);
  }
  /* As tall as its lines, scrolling once they fill the tab (so the echo
     toggle sits right under them). */
  .log {
    flex: 0 1 auto;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .turn .who {
    display: block;
    margin-bottom: 2px;
    font-size: 12px;
    font-weight: 650;
  }
  .turn.you .who {
    color: var(--yap-primary);
  }
  .turn.them .who {
    color: var(--them);
  }
  .turn p {
    margin: 0 0 3px;
    font-size: 14px;
    line-height: 1.6;
    overflow-wrap: anywhere;
    user-select: text;
    -webkit-user-select: text;
  }
  .turn.echo {
    opacity: 0.55;
  }
  .turn.echo p {
    font-style: italic;
  }
  .echonote {
    align-self: flex-start;
    margin-top: 10px;
    padding: 0;
    border: none;
    background: none;
    color: var(--yap-muted);
    font: inherit;
    font-size: 11.5px;
    cursor: pointer;
  }
  .echonote:hover {
    color: var(--yap-fg);
    text-decoration: underline;
  }

  /* ---- summary ---- */
  .summary {
    display: flex;
    flex-direction: column;
  }
  .steps {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 2px 0 12px;
  }
  .stepdot {
    flex: 0 0 auto;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--yap-fg);
    animation: recpulse 1.2s ease-in-out infinite;
  }
  .stepline {
    flex: 1 1 auto;
    font-size: 13px;
  }
  .stepcount {
    flex: 0 0 auto;
    font-size: 11.5px;
    color: var(--yap-muted-55);
    font-variant-numeric: tabular-nums;
  }
  .sumerr {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    margin-bottom: 14px;
    padding: 11px 12px;
    border: 1px solid color-mix(in srgb, var(--yap-warning) 35%, transparent);
    border-radius: var(--yap-r);
    background: color-mix(in srgb, var(--yap-warning) 8%, transparent);
    color: var(--yap-warning);
  }
  .sumerr > svg {
    flex: 0 0 auto;
    width: 15px;
    height: 15px;
    margin-top: 1px;
  }
  .errtext {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-width: 0;
    font-size: 13px;
  }
  .errdetail {
    font-size: 11.5px;
    color: var(--yap-muted);
    overflow-wrap: anywhere;
  }
  .retry {
    flex: 0 0 auto;
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 26px;
    padding: 0 10px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r-full);
    background: var(--yap-s2);
    color: var(--yap-fg);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .retry:hover {
    border-color: var(--yap-border-hover);
  }
  .retry svg {
    width: 12px;
    height: 12px;
  }
  .aicard {
    margin-bottom: 14px;
    padding: 12px 14px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg);
    background: var(--yap-primary-wash);
  }
  .aicard p {
    margin: 0 0 10px;
    font-size: 12.5px;
    line-height: 1.55;
  }
  .sumnote {
    margin: 0 0 12px;
    font-size: 12.5px;
    color: var(--yap-muted);
  }
  .sumhead {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-bottom: 4px;
  }
  .sumcap,
  .sumempty,
  .sofarcap {
    font-size: 10.5px;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--yap-muted-55);
  }
  .sumempty {
    margin: 2px 0 0;
  }
  .grow {
    flex: 1;
  }
  .copy {
    border: none;
    background: none;
    padding: 0;
    color: var(--yap-primary);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }
  .copy:hover {
    text-decoration: underline;
  }
  .rendered {
    font-size: 13.5px;
    line-height: 1.65;
    user-select: text;
    -webkit-user-select: text;
  }
  /* renderMarkdown shifts levels: "## Action plan" is an h3, "### Alice" an
     h4. Sections in the display serif, people in the UI sans. */
  .rendered :global(h2),
  .rendered :global(h3),
  .rendered :global(h4),
  .rendered :global(h5) {
    margin: 16px 0 6px;
    line-height: 1.3;
  }
  .rendered :global(h2),
  .rendered :global(h3) {
    font-family: var(--yap-font-display);
    font-size: 20px;
    font-weight: 500;
  }
  .rendered :global(h4),
  .rendered :global(h5) {
    margin-top: 12px;
    font-size: 13.5px;
    font-weight: 650;
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
    margin-top: 16px;
    font-size: 12.5px;
    line-height: 1.55;
  }
  .sofarcap {
    margin: 0 0 4px;
  }
  .sofar ul {
    margin: 0 0 8px;
    padding-left: 18px;
  }
  .sofar li.task {
    list-style: none;
    margin-left: -14px;
  }

  /* ---- "What did I miss?" ---- */
  .miss {
    flex: 0 1 auto;
    display: flex;
    flex-direction: column;
    max-height: 46%;
    min-height: 150px;
    margin: 0 14px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
    box-shadow: var(--yap-shadow-sm);
    overflow: hidden;
  }
  .misshead {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 8px 4px 14px;
    font-size: 10.5px;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--yap-muted-55);
  }
  .missx {
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
  }
  .missx:hover {
    background: var(--yap-s3);
    color: var(--yap-fg);
  }
  .missx svg {
    width: 11px;
    height: 11px;
  }
  .missthread {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 4px 14px 10px;
  }
  .mq {
    align-self: flex-end;
    max-width: 80%;
    margin: 0;
    padding: 6px 11px;
    border-radius: 12px;
    background: var(--yap-primary-wash);
    font-size: 13px;
  }
  .ma {
    align-self: flex-start;
    max-width: 92%;
    margin: 0;
    font-size: 13px;
    line-height: 1.55;
    user-select: text;
    -webkit-user-select: text;
  }
  .ma :global(p) {
    margin: 2px 0;
  }
  .ma :global(ul) {
    margin: 2px 0;
    padding-left: 18px;
  }
  .ma.err {
    color: var(--yap-danger);
  }
  .ma.busy {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    color: var(--yap-muted);
  }
  .ma.setup p {
    margin: 0 0 8px;
    color: var(--yap-fg);
  }
  .missask {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 8px 8px 12px;
    border-top: 1px solid var(--yap-border-subtle);
  }
  .missask input {
    flex: 1 1 auto;
    min-width: 0;
    height: 30px;
    border: none;
    background: transparent;
    color: var(--yap-fg);
    font: inherit;
    font-size: 13px;
  }
  .missask input:focus {
    outline: none;
  }
  .missask input::placeholder {
    color: var(--yap-muted-55);
  }
  .send {
    display: inline-flex;
    width: 28px;
    height: 28px;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: 50%;
    background: var(--yap-ink);
    color: var(--yap-ink-fg);
    cursor: pointer;
  }
  .send:disabled {
    opacity: 0.35;
    cursor: default;
  }
  .send svg {
    width: 13px;
    height: 13px;
  }

  /* ---- footer ---- */
  .foot {
    flex: 0 0 auto;
    padding: 10px 18px 14px;
  }
  .consent {
    margin: 0 0 9px;
    text-align: center;
    font-size: 11px;
    color: var(--yap-muted-55);
  }
  .footrow {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .pill {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    height: 34px;
    padding: 0 15px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r-full);
    background: var(--yap-s2);
    color: var(--yap-fg);
    font: inherit;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
    transition:
      border-color var(--yap-dur) ease,
      background var(--yap-dur) ease;
  }
  .pill:hover:not(:disabled) {
    border-color: var(--yap-border-hover);
  }
  .pill:disabled {
    opacity: 0.55;
    cursor: default;
  }
  .pill svg {
    width: 13px;
    height: 13px;
  }
  .sq {
    width: 10px;
    height: 10px;
    border-radius: 2.5px;
    background: var(--yap-danger);
  }
  .rdot {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    border: 2px solid var(--yap-danger);
    box-sizing: border-box;
    background: radial-gradient(circle, var(--yap-danger) 45%, transparent 50%);
  }
  .pill.gen {
    border-color: transparent;
    background: var(--yap-ink);
    color: var(--yap-ink-fg);
  }
  .pill.gen:hover {
    background: var(--yap-ink-hover);
  }
  .missbtn {
    height: 34px;
    padding: 0 6px;
    border: none;
    background: none;
    color: var(--yap-fg);
    font: inherit;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
  }
  .missbtn:hover:not(:disabled) {
    text-decoration: underline;
  }
  .missbtn:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .ink {
    height: 30px;
    padding: 0 13px;
    border: none;
    border-radius: var(--yap-r);
    background: var(--yap-ink);
    color: var(--yap-ink-fg);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .ink.small {
    height: 27px;
    font-size: 11.5px;
  }
  .ink:hover {
    background: var(--yap-ink-hover);
  }
</style>
