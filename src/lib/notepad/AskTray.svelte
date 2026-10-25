<script module>
  // What the person has seen, per note (transcript segments), for the
  // window's whole life: everything, whenever the transcript is on screen
  // and scrolled to its newest line. "What did I miss?" answers from there.
  const seen = new Map();
</script>

<script>
  // The bar at the bottom of the notepad: Stop / Resume, and "Ask anything",
  // which opens a bottom sheet chat grounded in the meeting
  // (meetings/meeting_assist.rs; "What did I miss?" answers from what was said since
  // you last looked: the transcript on screen at its newest line, or the last
  // answer). One-tap helpers sit in the sheet (the lead one in the pill while
  // it's down): What did I miss? and Questions to ask while recording,
  // Follow-up email and Gaps and risks after (`meetingHelpers`). Answers are
  // streamed (chat/stream.rs, core/streaming.js), with ■ to stop one, and can be
  // copied, read aloud and rated (kept on this PC only). The thread is kept
  // with the meeting (chat/note_chats.rs, core/noteChat.js): it's there again
  // when the note opens, here or in Notes, each turn saved as it's shown;
  // New chat starts another, and Past chats reopens an earlier one.
  import { tick, onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { renderMarkdown } from '../core/markdown.js';
  import { loadNoteChat, openNoteChat, keepTurn, keptMessage, threadMessages, pastChatItems } from '../core/noteChat.js';
  import { showSaveError } from '../notices/saveErrors.js';
  import { nextModel, modelLine, whereItRuns } from '../core/aiModel.js';
  import { messageItems } from '../core/itemMenus.js';
  import { MEETING_HELPERS, meetingHelpers } from '../core/notepadText.js';
  import { streamCall, liveAnswer, applyEvent, closePartial, settle, streamingHeight, STOPPED } from '../core/streaming.js';
  import { speech, readAloud } from '../stores/speech.svelte.js';
  import { openContextMenu, menuKey } from '../ui/contextMenu.svelte.js';
  import { copyToClipboard, openSettings, LABEL } from './actions.js';
  import Icon from './NotepadIcon.svelte';

  /**
   * `open`: the sheet is up (the notepad puts it away on Esc, a click above
   * it, another note); `transcriptShown`: the Transcript tab is up, at its
   * newest line; `windowShown`: the notepad is on screen; `meetingRecording`
   * / `stopping`: any meeting recording, and this one stopping;
   * `onstop()` / `onresume()`: the round button.
   */
  let {
    note,
    recordingThis,
    meetingRecording,
    stopping,
    open = $bindable(),
    transcriptShown,
    windowShown,
    onstop,
    onresume,
  } = $props();

  const lineCount = $derived((note?.transcript || []).length);
  $effect(() => {
    const n = lineCount;
    if (note && transcriptShown && windowShown && document.visibilityState === 'visible') {
      seen.set(note.id, Math.max(seen.get(note.id) ?? 0, n));
    }
  });

  let askThread = $state([]); // { id, role: 'user' | 'assistant', text, error?, plain?, streaming?, stopped? }
  let askBusy = $state(false);
  // The answer being written (chat/stream.rs): its ■, and whether it was pressed.
  let askStop = null;
  let askStopping = $state(false);
  const askStreaming = $derived(askThread.some((m) => m.streaming));
  let askBusyText = $state(''); // what the busy line says
  let askSetup = $state(null); // the "set up an AI model" reason
  // The one-tap helpers on offer, by the recording state (notepadText.js).
  const helpers = $derived(meetingHelpers(note, recordingThis));
  let askInput = $state('');
  let askFocused = $state(false);
  let askEl = $state(null);
  let threadEl = $state(null);
  let askSeq = 0;

  function openAsk() {
    if (open) return;
    open = true;
    scrollThread();
  }
  /** Put the sheet away (Esc, a click above it, the handle, −). */
  export function close() {
    open = false;
    askEl?.blur();
  }
  /** Another note: its own conversation (its kept thread follows, below). */
  export function reset() {
    askThread = [];
    askSetup = null;
    threadId = null;
    threads = [];
  }
  /** New chat: another thread from the next question (this one stays, under Past chats). */
  function newChat() {
    askThread = [];
    askSetup = null;
    threadId = null;
    askInput = '';
    fitAsk();
    askEl?.focus();
  }

  // ---- the thread kept with the meeting (chat/note_chats.rs) ----

  // The thread shown (null: none yet, or New chat) and the note's threads.
  let threadId = $state(null);
  let threads = $state([]);
  let threadFor = null;
  $effect(() => {
    const id = note?.id ?? null;
    if (id === threadFor) return;
    threadFor = id;
    reset();
    if (id != null) loadThread(id);
  });
  /** Note `id`'s newest thread, or thread `conversation` of it. */
  async function loadThread(id, conversation = null) {
    try {
      const chat = await loadNoteChat(id);
      const c = conversation != null ? await openNoteChat(id, conversation) : chat.current;
      if (note?.id !== id || askBusy) return;
      threads = chat.threads || [];
      threadId = c?.id ?? null;
      askThread = threadMessages(c, 'np');
      scrollThread();
    } catch {
      /* none kept: a new thread */
    }
  }
  /** Keep a turn as shown; a new thread's id comes back. */
  async function keep(id, messages) {
    try {
      const conv = await keepTurn(id, threadId, messages.map(keptMessage), LABEL);
      if (note?.id !== id) return;
      threadId = conv;
      threads = (await loadNoteChat(id)).threads || threads;
    } catch (e) {
      // Kept in memory and saved with the next change (shell/saving.rs).
      showSaveError(e);
    }
  }
  /** Past chats: the note's threads, the one shown ticked. */
  function pastChats(e) {
    const id = note?.id;
    if (id == null) return;
    openContextMenu(
      e,
      pastChatItems(threads, threadId, (conv) => loadThread(id, conv)),
      { label: 'Past chats' }
    );
  }
  onMount(() => {
    let un = null;
    let gone = false;
    // The Notes view added to this meeting's thread: show it, when it's
    // the one here (or nothing is).
    listen('yap-note-chat', (e) => {
      const p = e.payload || {};
      if (p.origin === LABEL || p.noteId !== note?.id || askBusy) return;
      if (threadId === p.conversationId || (threadId == null && !askThread.length)) loadThread(p.noteId, p.conversationId);
      else loadNoteChat(p.noteId).then((c) => note?.id === p.noteId && (threads = c.threads || [])).catch(() => {});
    }).then((u) => (gone ? u() : (un = u)));
    return () => {
      gone = true;
      un?.();
    };
  });

  /** Ask: no `question` = "What did I miss?" (since you last looked). */
  function ask(question = null) {
    if (!note || askBusy) return;
    const id = note.id;
    const q = question?.trim() || null;
    // Asking with the transcript on screen, at its newest line: that's seen.
    if (!q && transcriptShown) {
      seen.set(id, Math.max(seen.get(id) ?? 0, lineCount));
    }
    return converse(
      q ?? MEETING_HELPERS.catchUp.label,
      q ? 'Looking through the meeting…' : MEETING_HELPERS.catchUp.busy,
      (onEvent) => streamCall('meeting_catch_up', { noteId: id, since: seen.get(id) ?? 0, question: q }, onEvent),
      (r) => !q && seen.set(id, Math.max(seen.get(id) ?? 0, r.upto))
    );
  }

  /** A one-tap helper (a chip): "What did I miss?", or one of meeting_assist's. */
  function runHelper(kind) {
    if (kind === 'catchUp') return ask(null);
    if (!note || askBusy) return;
    const id = note.id;
    return converse(MEETING_HELPERS[kind].label, MEETING_HELPERS[kind].busy, (onEvent) =>
      streamCall('meeting_assist', { noteId: id, kind }, onEvent)
    );
  }

  /**
   * One turn in the sheet: `question` as the person's bubble, then the
   * answer `start(onEvent)` streams ({promise, stop}: streaming.js) — it
   * grows in place as it's written, and the command's own `{answer, plain?,
   * stopped?}` takes its place at the end (or what went wrong; no AI model =
   * the setup card).
   */
  async function converse(question, busyText, start, after = null) {
    if (!note || askBusy) return;
    const id = note.id;
    openAsk();
    askSetup = null;
    askThread = [...askThread, { id: `${Date.now()}-${++askSeq}`, role: 'user', text: question }];
    askBusy = true;
    askBusyText = busyText;
    askStopping = false;
    scrollThread();
    let live = liveAnswer();
    let aid = null;
    const onEvent = (ev) => {
      if (note?.id !== id) return;
      live = applyEvent(live, ev);
      if (!live.text) {
        if (ev?.kind === 'waiting') askBusyText = live.waiting ? 'Waiting for your dictation to finish…' : busyText;
        return;
      }
      if (aid == null) {
        aid = `${Date.now()}-${++askSeq}`;
        askThread = [...askThread, { id: aid, role: 'assistant', text: '', streaming: true }];
      }
      const m = askThread.find((x) => x.id === aid);
      if (m) m.text = live.text;
      followAsk();
    };
    const call = start(onEvent);
    askStop = call.stop;
    try {
      // Who answers (the Note Formatting scope, else cleanup), kept with it;
      // "Nothing new" and "too little to go on" come from Yap itself.
      const [r, answeredBy] = await Promise.all([call.promise, nextModel('notes')]);
      if (note?.id !== id) return;
      const model = r.nothingNew || r.tooLittle ? null : answeredBy;
      // The finished answer takes the streamed one's place (the email
      // cleaned up as plain text): a soft glide if it changed.
      const streamed = aid ? askThread.find((x) => x.id === aid)?.text : null;
      const from = streamingHeight(threadEl);
      const answer = {
        id: aid ?? `${Date.now()}-${++askSeq}`,
        role: 'assistant',
        text: r.answer,
        plain: !!r.plain,
        model,
        stopped: !!r.stopped,
        revisedFrom: streamed && streamed.trim() !== String(r.answer).trim() ? from : 0,
      };
      askThread = aid ? askThread.map((x) => (x.id === aid ? answer : x)) : [...askThread, answer];
      after?.(r);
      // Kept with the meeting, as shown (chat/note_chats.rs).
      await keep(id, [{ role: 'user', text: question }, answer]);
    } catch (e) {
      if (note?.id !== id) return;
      if (aid) askThread = askThread.filter((x) => x.id !== aid);
      const msg = String(e);
      if (askStopping && msg.includes(STOPPED)) {
        askThread = [...askThread, { id: `${Date.now()}-${++askSeq}`, role: 'assistant', text: 'Stopped.', quiet: true }];
        // The question stays with the meeting; nothing came to keep.
        await keep(id, [{ role: 'user', text: question }]);
      } else if (/No AI model configured|API key/i.test(msg)) askSetup = msg;
      else
        askThread = [
          ...askThread,
          { id: `${Date.now()}-${++askSeq}`, role: 'assistant', text: `I couldn't answer that: ${msg}`, error: true },
        ];
    } finally {
      askBusy = false;
      askStop = null;
      askStopping = false;
      scrollThread();
    }
  }
  /** ■: stop the answer being written (what came stays). */
  function stopAsk() {
    if (!askBusy || askStopping || !askStop) return;
    askStopping = true;
    askStop();
  }
  /** Keep the newest words in view while they're written, unless you scrolled up. */
  function followAsk() {
    const el = threadEl;
    if (el && el.scrollHeight - el.scrollTop - el.clientHeight < 120) tick().then(() => (el.scrollTop = el.scrollHeight));
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

  // An answer's right-click menu in the Ask sheet: Copy, Copy as Markdown,
  // Read aloud (not while this meeting records, as its ▶), then the ratings.
  function answerMenu(e, m) {
    if (m.error) return;
    const id = `notepad:${m.id}`;
    const items = messageItems(
      { role: 'assistant', text: m.text, plain: m.plain },
      { read: recordingThis ? null : () => readAloud(m.text, id), reading: speech.speakingId === id }
    );
    items.push({ separator: true });
    items.push({ label: 'Good answer', toggle: true, checked: feedback[m.id]?.rating === 'up', onselect: () => rate(m, 'up') });
    items.push({ label: 'Bad answer', toggle: true, checked: feedback[m.id]?.rating === 'down', onselect: () => rate(m, 'down') });
    // The focus comes back to the button the keys were pressed on, else the
    // answer's copy button.
    const row = e.type === 'keydown' ? e.target : e.currentTarget.querySelector('button[aria-label="Copy answer"]');
    openContextMenu(e, items, { label: 'Answer', row });
  }
  function questionMenu(e, m) {
    openContextMenu(e, messageItems({ role: 'user', text: m.text }), { label: 'Question', row: e.currentTarget });
  }
</script>

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

<div class="tray" class:open>
  {#if open}
    <div class="sheet" role="dialog" aria-label="Ask about this meeting">
      <div class="sheethead">
        <!-- The grab handle: a mouse target; the − button is the accessible one. -->
        <button class="handle" aria-hidden="true" tabindex="-1" onclick={close}><span></span></button>
        <div class="sheetbtns">
          {#if threads.length > 1 || (threads.length === 1 && threads[0].id !== threadId)}
            <button class="sbtn" aria-label="Past chats" data-tip="Past chats" data-more onclick={pastChats}><Icon name="clock" /></button>
          {/if}
          <button class="sbtn" aria-label="New chat" data-tip="New chat" onclick={newChat}><Icon name="newchat" /></button>
          <button class="sbtn" aria-label="Minimise chat" data-tip="Minimise" onclick={close}><Icon name="min" /></button>
        </div>
      </div>
      <div class="thread" bind:this={threadEl} aria-live="polite">
        {#if !askThread.length && !askSetup && !askBusy}
          <p class="threadhint">
            Ask anything about this meeting. Yap answers from the transcript and your notes.
            {#if recordingThis}
              What did I miss? catches you up on what was said since you last looked, and Questions to ask
              suggests what to ask next.
            {:else if helpers.length}
              Follow-up email drafts a recap to send, and Gaps and risks finds what the meeting left open.
            {/if}
          </p>
        {/if}
        {#each askThread as m (m.id)}
          {#if m.role === 'user'}
            <!-- Focusable for its menu from the keyboard (the ContextMenu key, Shift+F10). -->
            <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
            <p
              class="mq"
              tabindex="0"
              oncontextmenu={(e) => questionMenu(e, m)}
              onkeydown={(e) => e.target === e.currentTarget && menuKey(e, (k) => questionMenu(k, m))}
            >{m.text}</p>
          {:else}
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div
              class="ma"
              class:streaming={m.streaming}
              aria-busy={m.streaming ? 'true' : undefined}
              data-testid={m.streaming ? 'ask-streaming' : undefined}
              use:settle={m.revisedFrom || 0}
              oncontextmenu={(e) => answerMenu(e, m)}
              onkeydown={(e) => menuKey(e, (k) => answerMenu(k, m))}
            >
              {#if m.plain}
                <!-- An email: shown exactly as it copies. -->
                <div class="mabody plain">{m.text}</div>
              {:else}
                <!-- renderMarkdown passes no raw HTML and sanitises its output;
                     while it's written, half-written markdown is closed off. -->
                <div class="mabody" class:err={m.error} class:quiet={m.quiet}>{@html renderMarkdown(m.streaming ? closePartial(m.text) : m.text)}</div>
              {/if}
              {#if !m.streaming && !m.error && !m.quiet}
                <div class="mabtns">
                  <button class="fb" class:on={feedback[m.id]?.rating === 'up'} aria-label="Good answer" aria-pressed={feedback[m.id]?.rating === 'up'} onclick={() => rate(m, 'up')}><Icon name="up" /></button>
                  <button class="fb" class:on={feedback[m.id]?.rating === 'down'} aria-label="Bad answer" aria-pressed={feedback[m.id]?.rating === 'down'} onclick={() => rate(m, 'down')}><Icon name="down" /></button>
                  <button class="fb" aria-label="Copy answer" onclick={() => copyAnswer(m)}><Icon name={copiedAnswer === m.id ? 'check' : 'copy'} /></button>
                  <!-- ▶ Read aloud (voice/tts.rs): not while this meeting records — the notes would hear it. -->
                  {#if !recordingThis}
                    {@const reading = speech.speakingId === `notepad:${m.id}`}
                    <button class="fb" class:on={reading} aria-label={reading ? 'Stop reading' : 'Read aloud'} aria-pressed={reading} onclick={() => readAloud(m.text, `notepad:${m.id}`)}><Icon name={reading ? 'stop' : 'play'} /></button>
                  {/if}
                  {#if m.model}
                    <span class="mamodel" title={whereItRuns(m.model)}>{modelLine(m.model)}</span>
                  {/if}
                  {#if m.stopped}
                    <span class="mamodel" title="You stopped this answer while it was being written">· Stopped</span>
                  {/if}
                </div>
              {/if}
            </div>
          {/if}
        {/each}
        {#if askBusy && !askStreaming}
          <p class="ma busy"><span class="dots" aria-hidden="true"><i></i><i></i><i></i></span>{askBusyText}</p>
        {/if}
        {#if askSetup}
          <div class="ma setup">
            <p>Yap needs an AI model to answer. Set one up in Language Models; an on-device model keeps everything on this PC.</p>
            <button class="ink small" onclick={() => openSettings('cleanup')}>Open Language Models</button>
          </div>
        {/if}
      </div>
      <!-- The one-tap helpers, out of the way while a question is typed. -->
      {#if helpers.length && !askInput.trim()}
        <div class="helpers" role="group" aria-label="Suggestions">
          {#each helpers as kind (kind)}
            <button class="chip" onclick={() => runHelper(kind)} disabled={askBusy}>{MEETING_HELPERS[kind].label}</button>
          {/each}
        </div>
      {/if}
    </div>
  {/if}
  <div class="barrow">
    <button
      class="rec"
      class:circle={open}
      aria-label={recordingThis ? (stopping ? 'Stopping…' : 'Stop') : note.transcript?.length ? 'Resume' : 'Start'}
      disabled={(!recordingThis && meetingRecording) || stopping}
      title={!recordingThis && meetingRecording ? 'Yap is recording another meeting' : ''}
      onclick={() => (recordingThis ? onstop() : onresume())}
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
      {#if askBusy}
        <!-- ■: stop the answer being written. -->
        <button class="send" aria-label="Stop the answer" data-tip="Stop" onclick={stopAsk} disabled={askStopping}><Icon name="stop" /></button>
      {:else if askInput.trim()}
        <button class="send" aria-label="Ask" onclick={sendTyped} disabled={askBusy}><Icon name="send" /></button>
      {:else if helpers.length && !open}
        <!-- The lead helper (the sheet shows them all when it's up, and
             the focus goes to the field, as this chip goes away). -->
        <button
          class="chip"
          onclick={() => {
            runHelper(helpers[0]);
            askEl?.focus();
          }}
          disabled={askBusy}>{MEETING_HELPERS[helpers[0]].label}</button
        >
      {/if}
    </div>
  </div>
</div>

<style>
  button {
    font: inherit;
    cursor: pointer;
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
  /* (Their tooltips end at the sheet's edge: notepad.css.) */
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
  /* Thin scrollbars, as on the page they sit on (8px, a soft thumb). */
  .thread::-webkit-scrollbar {
    width: 8px;
  }
  .thread::-webkit-scrollbar-thumb {
    border-radius: 4px;
    background: color-mix(in srgb, var(--yap-paper-ink) 16%, transparent);
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
    font-family: var(--yap-font-reading);
    font-size: calc(15px * var(--yap-reading-scale));
    line-height: calc(1.4 * var(--yap-reading-leading));
    overflow-wrap: anywhere;
    user-select: text;
    -webkit-user-select: text;
    animation: bubblein 0.1s ease-out both;
  }
  .mq:focus-visible {
    outline: none;
    box-shadow: 0 0 0 3px var(--yap-paper-ring);
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
    font-family: var(--yap-font-reading);
    font-size: calc(15px * var(--yap-reading-scale));
    line-height: calc(1.5 * var(--yap-reading-leading));
    overflow-wrap: anywhere;
    user-select: text;
    -webkit-user-select: text;
  }
  .mabody.err {
    color: var(--yap-paper-danger);
  }
  .mabody.quiet {
    color: var(--yap-paper-muted);
    font-size: 13px;
  }
  /* An answer being written: a soft caret at the end of its last line. */
  .ma.streaming .mabody > :global(:is(p, h2, h3, h4, h5, blockquote):last-child)::after,
  .ma.streaming .mabody > :global(:is(ul, ol):last-child > li:last-child)::after {
    content: '';
    display: inline-block;
    width: 7px;
    height: 7px;
    margin-left: 5px;
    border-radius: 50%;
    background: var(--yap-live);
    vertical-align: middle;
    animation: askcaret 1s ease-in-out infinite;
  }
  @keyframes askcaret {
    0%,
    100% {
      opacity: 0.25;
    }
    50% {
      opacity: 0.9;
    }
  }
  :global(:root[data-motion='reduced']) .ma.streaming .mabody > :global(*)::after,
  :global(:root[data-motion='reduced']) .ma.streaming .mabody > :global(* > li)::after {
    animation: none;
  }
  .mabody.plain {
    white-space: pre-wrap;
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
    align-items: center;
    gap: 4px;
    margin-top: 4px;
  }
  /* the model that answered, quietly beside the buttons */
  .mamodel {
    margin-left: 6px;
    font-size: 11px;
    color: var(--yap-paper-date);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
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
  @keyframes breathe {
    50% {
      opacity: 0.35;
    }
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
    background: var(--yap-paper-card);
    color: var(--yap-paper-ink);
    font-size: 15px;
    font-weight: 600;
    transition: all 0.1s linear;
  }
  .rec:hover:not(:disabled) {
    border-color: var(--yap-paper-s2-strong);
    background: var(--yap-s1);
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
    background: var(--yap-paper-card);
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
  .chip:focus-visible {
    outline: none;
    box-shadow: 0 0 0 3px var(--yap-paper-ring);
  }
  /* The sheet's one-tap helpers, under the thread. */
  .helpers {
    flex: 0 0 auto;
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    padding: 4px 16px 0;
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
</style>
