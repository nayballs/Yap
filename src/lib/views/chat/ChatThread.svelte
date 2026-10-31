<script>
  // Chat's thread: the conversation's messages (ChatMessage), and at its
  // foot while an answer comes what the turn is doing now and for how long
  // (ActivityLine: "Searching the web · … · 1m 59s"; a model on this PC slow
  // to start says why), or — no messages yet — what Chat is for, what it
  // needs if no AI model is set up, and the Voice Assistant's shortcut (or
  // how to set one).
  import { formatHotkeySpec } from '../../core/hotkeys.js';
  import { messageItems } from '../../core/itemMenus.js';
  import { openContextMenu } from '../../ui/contextMenu.svelte.js';
  import { STILL_WORKING_S } from './chatState.svelte.js';
  import { SUMMARY_DIVIDER } from './chatBudget.js';
  import { skills } from '../../stores/skills.svelte.js';
  import { promptChips } from '../../core/skills.js';
  import ChatMessage from './ChatMessage.svelte';
  import ActivityLine from './ActivityLine.svelte';

  // The one-tap prompts (chat/skills.rs: Coach me, List my open to-dos,
  // Write a weekly recap), each asked across the meetings.
  const prompts = $derived(promptChips(skills.list));

  // The answer's words have begun (a slow start's hint goes).
  const answering = $derived(chat.messages.some((m) => m.streaming && m.text));

  // Show summary's button, the focus's way back when its dialog closes.
  let summaryBtn = $state(null);
  function closeSummary() {
    chat.showingSummary = false;
    summaryBtn?.focus({ preventScroll: true });
  }

  /** `chat`: the view's state; `onopensettings(section)`: ControlPanel's Settings. */
  let { chat, onopensettings = null } = $props();

  function messageMenu(e, m) {
    const reading = !!m.id && chat.speakingId === m.id;
    openContextMenu(
      e,
      messageItems(m, { read: m.role === 'assistant' && m.id ? () => chat.readAloud(m) : null, reading }),
      { label: m.role === 'assistant' ? 'Answer' : 'Question', row: e.currentTarget }
    );
  }
</script>

{#if chat.messages.length === 0}
  <div class="empty">
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" /><path d="M8 9h8M8 13h5" /></svg>
    {#if chat.scope === 'meetings'}
      <!-- Asked across the meetings (search/ask.rs): found on this PC, cited. -->
      <p class="across">Ask across your meetings and notes</p>
      <p class="asknote">Answers come from what was said and written, and cite the moment: click a number to open it.</p>
    {:else}
      <p>Ask about your notes, transcripts, or anything else</p>
      <p class="asknote">
        <button class="setkey" onclick={() => chat.askAcrossMeetings()}>Ask across your meetings</button>
        for answers that cite the moment they come from.
      </p>
    {/if}
    {#if prompts.length}
      <!-- One-tap prompts: the question filled in and sent, across the meetings. -->
      <div class="prompts" role="group" aria-label="Try asking">
        {#each prompts as p (p.id)}
          <button class="prompt" disabled={chat.sending} onclick={() => chat.runPrompt(p)}>{p.chip}</button>
        {/each}
      </div>
      <p class="asknote">Or type / for Skills, like /decisions or /risks.</p>
    {/if}
    {#if !chat.chatReady}
      <!-- Before a first question fails: what Chat needs. -->
      <p class="asknote needsai">
        Chat needs an AI model: a free key from a cloud provider, or one that runs on this PC.
        <button class="setkey" onclick={() => onopensettings?.('cleanup#chat')}>Set up an AI model</button>
      </p>
    {/if}
    <!-- The Voice Assistant: the same chat, asked out loud from any app. -->
    <p class="asknote">
      {#if chat.assistantHotkey}
        Or press <kbd>{formatHotkeySpec(chat.assistantHotkey)}</kbd> in any app to ask out loud and hear the answer.
      {:else}
        <button class="setkey" onclick={() => onopensettings?.('voice#assistant-shortcut')}>Set an assistant shortcut</button>
        to ask out loud from any app and hear the answer.
      {/if}
    </p>
  </div>
{:else}
  {#if chat.scope === 'meetings'}
    <!-- This conversation asks across the meetings, follow-ups too. -->
    <p class="scopeline" data-testid="chat-across-meetings">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="11" cy="11" r="7" /><path d="m20 20-3.5-3.5" /></svg>
      Asking across your meetings and notes
    </p>
  {/if}
  <div class="msgs" bind:this={chat.threadEl}>
    {#each chat.messages as m, i (i)}
      {#if chat.summary && i === chat.summary.upto}
        <!-- A long chat's older messages, summarised (chat/compact.rs): the
             model gets the summary in their place, the latest whole. -->
        <div class="summarised" role="separator" aria-label={SUMMARY_DIVIDER} data-testid="chat-summary-divider">
          <span class="rule"></span>
          <span class="stext">{SUMMARY_DIVIDER} ·</span>
          <button type="button" class="showsum" bind:this={summaryBtn} onclick={() => (chat.showingSummary = true)}>Show summary</button>
          <span class="rule"></span>
        </div>
      {/if}
      <ChatMessage
        {m}
        last={i === chat.messages.length - 1}
        sources={chat.citedSources(i)}
        reading={m.role === 'assistant' && !!m.id && chat.speakingId === m.id}
        keepDays={chat.keepDays}
        onview={(id) => (chat.viewing = id)}
        onmenu={(e) => messageMenu(e, m)}
        onread={() => chat.readAloud(m)}
        onsource={(s) => chat.openSource(s)}
        ondoc={(d) => chat.doc.show(d.id, { title: d.title })}
      />
    {/each}
    {#if chat.sending || (chat.assistantPending != null && chat.assistantPending === chat.activeId)}
      <!-- What the turn is doing now, with its time (chat/activity.rs). -->
      <ActivityLine
        step={chat.activity}
        waiting={chat.activityWaiting}
        seconds={chat.waitedS}
        hint={chat.modelHere && !answering && chat.waitedS >= STILL_WORKING_S
          ? 'Still working: the model runs on this PC, so this can take a minute.'
          : ''}
      />
    {/if}
  </div>
{/if}

{#if chat.showingSummary && chat.summary}
  <!-- Its own chunk, loaded the first time it opens. -->
  {#await import('./SummaryDialog.svelte') then { default: SummaryDialog }}
    <SummaryDialog summary={chat.summary} onclose={closeSummary} />
  {/await}
{/if}

<style>
  /* Where the summary takes over: a thin rule, quiet words, Show summary. */
  .summarised {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 6px 0;
    font-size: 11px;
    color: var(--yap-muted);
  }
  .summarised .rule {
    flex: 1 1 16px;
    height: 1px;
    background: var(--yap-border-subtle);
  }
  .summarised .stext {
    flex: 0 1 auto;
    text-align: center;
  }
  .showsum {
    flex: 0 0 auto;
    padding: 0;
    border: none;
    background: none;
    color: var(--yap-primary);
    font: inherit;
    font-weight: 600;
    cursor: pointer;
  }
  .showsum:hover {
    text-decoration: underline;
  }
  .showsum:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 2px;
    border-radius: 4px;
  }
  .empty {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 10px;
    text-align: center;
    color: var(--yap-muted-55);
  }
  .empty svg {
    width: 36px;
    height: 36px;
    opacity: 0.7;
  }
  .empty p {
    margin: 0;
    max-width: 220px;
    font-size: 12px;
    line-height: 1.6;
  }
  /* The Voice Assistant's line: a gentle way to its shortcut. */
  .empty .asknote {
    max-width: 280px;
    margin-top: 6px;
    color: var(--yap-muted);
  }
  .empty .asknote.needsai {
    max-width: 320px;
    color: var(--yap-fg-80);
  }
  .empty .across {
    max-width: 280px;
    color: var(--yap-fg);
    font-weight: 600;
  }
  /* The one-tap prompts on an empty chat. */
  .prompts {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 6px;
    max-width: 420px;
    margin-top: 6px;
  }
  .prompt {
    height: 28px;
    padding: 0 12px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-full);
    background: var(--yap-s1);
    color: var(--yap-fg);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    transition:
      background 0.15s,
      border-color 0.15s;
  }
  .prompt:hover:not(:disabled) {
    background: var(--yap-s2);
    border-color: var(--yap-border-hover);
  }
  .prompt:disabled {
    opacity: 0.55;
    cursor: default;
  }
  .prompt:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 2px;
  }
  /* A conversation asked across the meetings says so over its thread. */
  .scopeline {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0 0 8px;
    font-size: 11px;
    font-weight: 600;
    color: var(--yap-muted);
  }
  .scopeline svg {
    width: 12px;
    height: 12px;
    color: var(--yap-primary);
  }
  .asknote kbd {
    padding: 1px 6px;
    border: 1px solid var(--yap-key-border);
    border-radius: var(--yap-r-sm);
    background: var(--yap-key-bg);
    color: var(--yap-key-fg);
    font: inherit;
    font-size: 11px;
    font-weight: 650;
  }
  .setkey {
    padding: 0;
    border: none;
    background: none;
    color: var(--yap-primary);
    font: inherit;
    font-weight: 650;
    cursor: pointer;
  }
  .setkey:hover {
    text-decoration: underline;
  }
  .msgs {
    flex: 1 1 auto;
    overflow-y: auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 2px;
  }
</style>
