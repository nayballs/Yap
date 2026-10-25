<script>
  // The note's own chat (OpenWhispr's embedded "Ask anything…" bar,
  // useEmbeddedChat port): answers grounded in this note (note_ask, the
  // Chat scope), a meeting note's one-tap helpers (meetings/meeting_assist.rs:
  // Questions to ask while it records, Follow-up email and Gaps and risks
  // after; "What did I miss?" stays the notepad's: it knows what you have
  // seen), ▶ Read aloud and the model under each answer; the bar's mic
  // dictates the question, and at its right the action picker.
  import { invoke } from '@tauri-apps/api/core';
  import { renderMarkdown } from '../../core/markdown.js';
  import { nextModel, modelLine, whereItRuns } from '../../core/aiModel.js';
  import { messageItems } from '../../core/itemMenus.js';
  import { MEETING_HELPERS, meetingHelpers } from '../../core/notepadText.js';
  import { pastChatItems } from '../../core/noteChat.js';
  import { speech, readAloud } from '../../stores/speech.svelte.js';
  import { openContextMenu, menuKey } from '../../ui/contextMenu.svelte.js';
  import { toast } from '../../ui/toast.svelte.js';
  import ActionPicker from './ActionPicker.svelte';

  /** `notes`: the view's state; `onopensettings(section)`: ControlPanel's Settings. */
  let { notes, onopensettings = null } = $props();

  async function askNote() {
    const q = notes.chatInput.trim();
    if (!q || !notes.selected || notes.asking) return;
    notes.chatInput = '';
    notes.flushSave();
    const history = notes.chatThread.slice(-6).map((t) => [t.role, t.text]);
    const id = notes.selected.id;
    notes.chatThread = [...notes.chatThread, { role: 'user', text: q }];
    notes.asking = true;
    try {
      const [answer, model] = await Promise.all([invoke('note_ask', { id, question: q, history }), nextModel('chat')]);
      const reply = { role: 'assistant', text: answer, model, id: notes.turnId() };
      notes.chatThread = [...notes.chatThread, reply];
      // Kept with the note (chat/note_chats.rs): here and in the notepad next time.
      await notes.keepChat(id, [{ role: 'user', text: q }, reply]);
    } catch (e) {
      notes.chatThread = notes.chatThread.slice(0, -1);
      notes.chatInput = q; // give the question back
      toast({ title: "Couldn't answer", description: String(e), variant: 'destructive' });
    } finally {
      notes.asking = false;
    }
  }

  // A meeting note's one-tap helpers (meetings/meeting_assist.rs), as chips over the
  // bar: Questions to ask while it records, Follow-up email and Gaps and
  // risks after. ("What did I miss?" stays the notepad's: it knows what you
  // have seen.)
  const noteHelpers = $derived(
    notes.selected?.noteType === 'meeting'
      ? meetingHelpers(notes.selected, notes.recordingThisNote).filter((k) => k !== 'catchUp')
      : []
  );
  async function askHelper(kind) {
    if (!notes.selected || notes.asking) return;
    const id = notes.selected.id;
    notes.flushSave();
    notes.chatThread = [...notes.chatThread, { role: 'user', text: MEETING_HELPERS[kind].label }];
    notes.asking = true;
    try {
      const [r, model] = await Promise.all([invoke('meeting_assist', { noteId: id, kind }), nextModel('notes')]);
      if (notes.selected?.id !== id) return;
      // "Too little to go on" comes from Yap, not a model.
      const reply = { role: 'assistant', text: r.answer, plain: !!r.plain, copy: true, model: r.tooLittle ? null : model, id: notes.turnId() };
      notes.chatThread = [...notes.chatThread, reply];
      await notes.keepChat(id, [{ role: 'user', text: MEETING_HELPERS[kind].label }, reply]);
    } catch (e) {
      if (notes.selected?.id !== id) return;
      notes.chatThread = notes.chatThread.slice(0, -1);
      const msg = String(e);
      const setup = /No AI model configured|API key/i.test(msg) && onopensettings;
      toast({
        title: "Couldn't answer",
        description: msg,
        variant: 'destructive',
        ...(setup ? { action: { label: 'Open Language Models', onClick: () => onopensettings('cleanup') } } : {}),
      });
    } finally {
      notes.asking = false;
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

  // Past chats: the note's kept threads, the one shown ticked.
  function pastChats(e) {
    const id = notes.selected?.id;
    if (id == null) return;
    openContextMenu(e, pastChatItems(notes.chatThreads, notes.chatConvId, (conv) => notes.loadChat(id, conv)), { label: 'Past chats' });
  }

  // The bar's mic: focus the input and start dictation — Yap types wherever
  // the caret is, so the answer box IS the dictation target.
  let askEl = $state(null);
  function micAsk() {
    askEl?.focus();
    invoke('toggle_recording').catch(() => {});
  }
</script>

<!-- Embedded note chat thread (answers grounded in this note), kept with
     the note: New chat starts another, Past chats reopens one. -->
{#if notes.chatThread.length > 0 || notes.chatThreads.length > 0}
  <div class="chathead">
    <span class="chatcap">{notes.chatThread.length ? 'Asked about this note' : 'Ask about this note'}</span>
    <span class="tabspacer"></span>
    {#if notes.chatThreads.length > 1 || (notes.chatThreads.length === 1 && notes.chatThreads[0].id !== notes.chatConvId)}
      <button class="chatlink" data-more onclick={pastChats}>Past chats</button>
    {/if}
    {#if notes.chatThread.length}
      <button class="chatlink" onclick={notes.newChat} disabled={notes.asking}>New chat</button>
    {/if}
  </div>
{/if}
{#if notes.chatThread.length > 0}
  <div class="chatthread">
    {#each notes.chatThread as turn, i (i)}
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
    {#if notes.asking}
      <div class="cbubble assistant"><p class="cbody thinking">Thinking…</p></div>
    {/if}
  </div>
{/if}

{#if noteHelpers.length && !notes.chatInput.trim()}
  <div class="helperrow" role="group" aria-label="Meeting helpers">
    {#each noteHelpers as kind (kind)}
      <button class="helperchip" onclick={() => askHelper(kind)} disabled={notes.asking}>{MEETING_HELPERS[kind].label}</button>
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
    bind:value={notes.chatInput}
    placeholder="Ask anything…"
    aria-busy={notes.asking}
    onkeydown={(e) => e.key === 'Enter' && !e.isComposing && askNote()}
  />
  <!-- Not `disabled` while asking: that would throw the caret out of
       the box (askNote ignores Enter until the answer is in). -->
  <ActionPicker {notes} />
</div>

<style>
  /* the kept thread's head: New chat, Past chats */
  .chathead {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-top: 10px;
  }
  .chatcap {
    font-size: 11px;
    font-weight: 600;
    color: var(--yap-muted-70);
  }
  .tabspacer {
    flex: 1;
  }
  .chatlink {
    padding: 0;
    border: none;
    background: none;
    color: var(--yap-primary);
    font: inherit;
    font-size: 11.5px;
    cursor: pointer;
  }
  .chatlink:hover:not(:disabled) {
    text-decoration: underline;
  }
  .chatlink:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .chatlink:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 2px;
    border-radius: 4px;
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
  /* Under an answer: ▶ Read aloud (voice/tts.rs), then who answered. */
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
</style>
