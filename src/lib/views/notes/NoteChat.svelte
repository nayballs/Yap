<script>
  // The note's own chat (OpenWhispr's embedded "Ask anything…" bar,
  // useEmbeddedChat port): answers grounded in this note (note_ask, the
  // Chat scope), a meeting note's one-tap helpers (meetings/meeting_assist.rs:
  // Questions to ask while it records, Follow-up email and Gaps and risks
  // after; "What did I miss?" stays the notepad's: it knows what you have
  // seen), ▶ Read aloud and the model under each answer; the bar's mic
  // dictates the question, and at its right the action picker. On a
  // meeting note, "/" lists the Skills that run on one meeting
  // (chat/skills.rs, meeting_assist.rs `meeting_skill`); their answers, and
  // Make me sound smart's, cite the meeting's lines (a pill opens the line).
  import { invoke } from '@tauri-apps/api/core';
  import { renderMarkdown } from '../../core/markdown.js';
  import { nextModel, modelLine, whereItRuns } from '../../core/aiModel.js';
  import { messageItems } from '../../core/itemMenus.js';
  import { MEETING_HELPERS, meetingHelpers } from '../../core/notepadText.js';
  import { pastChatItems } from '../../core/noteChat.js';
  import { asked as skillAsked, withoutCites } from '../../core/skills.js';
  import { askReplaceSummary } from '../../core/noteEdits.js';
  import { SkillPicker } from '../../stores/skills.svelte.js';
  import SkillMenu from '../../components/SkillMenu.svelte';
  import { speech, readAloud } from '../../stores/speech.svelte.js';
  import { openContextMenu, menuKey } from '../../ui/contextMenu.svelte.js';
  import { toast } from '../../ui/toast.svelte.js';
  import ActionPicker from './ActionPicker.svelte';

  /** `notes`: the view's state; `onopensettings(section)`: ControlPanel's Settings. */
  let { notes, onopensettings = null } = $props();

  // "/" on a meeting note: the Skills that run on one meeting.
  const picker = new SkillPicker('meeting');
  const isMeeting = $derived(notes.selected?.noteType === 'meeting');
  $effect(() => picker.update(isMeeting ? notes.chatInput : ''));
  function onAskKeydown(e) {
    const picked = picker.key(e);
    if (picked) {
      if (picked.skill) pickSkill(picked.skill, picked.run);
      return;
    }
    if (e.key === 'Enter' && !e.isComposing) askNote();
  }
  function pickSkill(skill, run) {
    if (run) {
      notes.chatInput = '';
      runSkill(skill, '');
    } else {
      notes.chatInput = `/${skill.command} `;
      askEl?.focus();
    }
  }

  async function askNote() {
    const q = notes.chatInput.trim();
    if (!q || !notes.selected || notes.asking) return;
    // "/decisions …" on a meeting note: a Skill on that meeting.
    const run = isMeeting ? picker.parse(q) : null;
    if (run) {
      notes.chatInput = '';
      return runSkill(run.skill, run.words);
    }
    notes.chatInput = '';
    notes.flushSave();
    // The kept thread's earlier turns go from Rust, sized to the model (its
    // summary and the latest turns: chat/note_chats.rs); these only when
    // there's no kept thread yet.
    const history = notes.chatThread.slice(-6).map((t) => [t.role, t.text]);
    const id = notes.selected.id;
    const conversationId = notes.chatConvId ?? null;
    notes.chatThread = [...notes.chatThread, { role: 'user', text: q }];
    notes.asking = true;
    try {
      const [answer, model] = await Promise.all([invoke('note_ask', { id, question: q, history, conversationId }), nextModel('chat')]);
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
  function askHelper(kind) {
    if (kind === 'longer') return makeLonger();
    return runAssist(MEETING_HELPERS[kind].label, (id) => invoke('meeting_assist', { noteId: id, kind }));
  }
  /** A Skill on this meeting, with the person's words. */
  function runSkill(skill, words) {
    return runAssist(skillAsked(skill, words), (id) => invoke('meeting_skill', { noteId: id, command: skill.command, words: words || null }));
  }
  /** "Make notes longer": the summary rewritten in more detail, Undo in the toast (an edited one asked first). */
  function makeLonger() {
    if (!notes.selected || notes.asking) return;
    if (notes.selected.summaryEditedMs) askReplaceSummary({ run: () => longer(true) });
    else longer(false);
  }
  async function longer(replace) {
    if (!notes.selected || notes.asking) return;
    const id = notes.selected.id;
    const label = MEETING_HELPERS.longer.label;
    notes.flushSave();
    notes.chatThread = [...notes.chatThread, { role: 'user', text: label }];
    notes.asking = true;
    try {
      const [r, model] = await Promise.all([invoke('meeting_longer', { noteId: id, replace, origin: 'longer' }), nextModel('notes')]);
      if (notes.selected?.id !== id) return;
      const reply = { role: 'assistant', text: 'Your notes are longer now, with more detail from the meeting. They are on the Summary tab.', model, id: notes.turnId() };
      notes.chatThread = [...notes.chatThread, reply];
      if (notes.selected.enhancedContent) notes.tab = 'enhanced';
      toast({
        title: 'Notes made longer',
        description: 'Yap rewrote the summary in more detail from the meeting.',
        action: {
          label: 'Undo',
          onClick: () =>
            invoke('note_summary_put_back', { id, before: r.previous, origin: 'longer' }).catch((e) =>
              toast({ title: "Couldn't put the summary back", description: String(e), variant: 'destructive' })
            ),
        },
      });
      await notes.keepChat(id, [{ role: 'user', text: label }, reply]);
    } catch (e) {
      if (notes.selected?.id !== id) return;
      notes.chatThread = notes.chatThread.slice(0, -1);
      failed(e);
    } finally {
      notes.asking = false;
    }
  }
  /** A cited line of the meeting (a pill in an answer), opened here. */
  function clickAnswer(e, turn) {
    const a = e.target instanceof Element ? e.target.closest('a.md-passage') : null;
    if (!a) return;
    e.preventDefault();
    const s = (turn.sources || []).find((x) => x.n === Number(a.textContent));
    if (!s || s.note !== notes.selected?.id) return;
    if (s.field === 'summary') notes.tab = 'enhanced';
    else if (s.field === 'content') notes.tab = 'raw';
    else {
      const lines = notes.selected.transcript || [];
      const at = lines[s.line]?.ts === s.ts || !s.ts ? s.line : lines.findIndex((x) => x.ts === s.ts);
      if (at >= 0) notes.showLine(at);
    }
  }
  function failed(e) {
    const msg = String(e);
    const setup = /No AI model configured|API key/i.test(msg) && onopensettings;
    toast({
      title: "Couldn't answer",
      description: msg,
      variant: 'destructive',
      ...(setup ? { action: { label: 'Open Language Models', onClick: () => onopensettings('cleanup') } } : {}),
    });
  }
  async function runAssist(question, call) {
    if (!notes.selected || notes.asking) return;
    const id = notes.selected.id;
    notes.flushSave();
    notes.chatThread = [...notes.chatThread, { role: 'user', text: question }];
    notes.asking = true;
    try {
      const [r, model] = await Promise.all([call(id), nextModel('notes')]);
      if (notes.selected?.id !== id) return;
      // "Too little to go on" comes from Yap, not a model.
      const reply = { role: 'assistant', text: r.answer, plain: !!r.plain, copy: true, model: r.tooLittle ? null : model, id: notes.turnId(), sources: r.sources || [] };
      notes.chatThread = [...notes.chatThread, reply];
      await notes.keepChat(id, [{ role: 'user', text: question }, reply]);
    } catch (e) {
      if (notes.selected?.id !== id) return;
      notes.chatThread = notes.chatThread.slice(0, -1);
      failed(e);
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
      // An answer citing the meeting copies without its "[2]"s.
      await navigator.clipboard.writeText(turn.sources?.length ? withoutCites(turn.text) : turn.text);
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
          <!-- renderMarkdown sanitises its output; a cited line's "[2]" is a
               pill that opens it (Make me sound smart, a Skill). -->
          <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
          <div class="cbody" onclick={(e) => clickAnswer(e, turn)}>{@html renderMarkdown(turn.text, turn.sources?.length ? { sources: turn.sources } : {})}</div>
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
  {#if isMeeting}
    <!-- "/": the Skills that run on one meeting. -->
    <SkillMenu {picker} onpick={pickSkill} />
  {/if}
  <input
    class="askinput"
    bind:this={askEl}
    bind:value={notes.chatInput}
    placeholder="Ask anything…"
    title={isMeeting ? 'Type / for Skills' : undefined}
    aria-busy={notes.asking}
    aria-autocomplete={isMeeting ? 'list' : undefined}
    aria-controls={picker.open ? 'skill-list' : undefined}
    aria-activedescendant={picker.open ? `skill-opt-${picker.active}` : undefined}
    onkeydown={onAskKeydown}
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
  /* A cited line's pill in an answer (opens the line). */
  .cbody :global(.md-cites) {
    display: inline-flex;
    gap: 2px;
    margin-left: 2px;
    vertical-align: 1px;
  }
  .cbody :global(a.md-cite) {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 15px;
    height: 15px;
    padding: 0 4px;
    box-sizing: border-box;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-full);
    background: var(--yap-s1);
    color: var(--yap-muted);
    font-size: 9.5px;
    font-weight: 650;
    line-height: 1;
    text-decoration: none;
  }
  .cbody :global(a.md-cite:hover) {
    color: var(--yap-primary);
  }
  .askbar {
    position: relative;
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
