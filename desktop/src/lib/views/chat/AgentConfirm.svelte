<script>
  // **Yap asks before sending an agent** (chat/tools/agents.rs, a `confirm`
  // card with act "agent"; chat/pages/act/confirm.rs): a folder no agent has
  // worked in, or a
  // chat that read the web (a page could have slipped words into the
  // brief). It reads as an agent, not a page: the folder and its path, the
  // mode it would run in, the brief in full (it scrolls), and why Yap asks.
  // **Send** / **Don't** (`chat_page_confirm`); no answer in two minutes is
  // a no. Kept with the answer, answered (ChatMessage.confirms). In
  // PageConfirm's chunk.
  import { invoke } from '@tauri-apps/api/core';
  import { questionParts, whyOf } from './pageConfirm.js';

  /** `card`: the confirm event (`{id, label, site, state, text, agent: {folder, path, mode, modeLabel, brief, newFolder, web}}`). */
  let { card } = $props();

  // Answered here, before Rust's next event says so.
  let answered = $state('');
  const st = $derived((card.state === 'asking' && answered) || card.state);
  const q = $derived(questionParts(card));
  const j = $derived(card.agent ?? {});
  const brief = $derived(j.brief || card.text || '');
  const path = $derived(j.path || card.site || '');
  const bypass = $derived(j.mode === 'bypassPermissions');

  async function answer(allow) {
    answered = allow ? 'allowed' : 'refused';
    try {
      await invoke('chat_page_confirm', { id: card.id, allow });
    } catch {
      /* the question had lapsed (the turn stopped): its card says no */
    }
  }

  const done = (s) =>
    j.continues
      ? s === 'allowed'
        ? 'Sent to the agent.'
        : s === 'timedOut'
          ? "No answer in time, so Yap didn't send it."
          : "Yap didn't send it."
      : s === 'allowed'
        ? 'Sent.'
        : s === 'timedOut'
          ? "No answer in time, so Yap didn't send it."
          : "Yap didn't send it.";
</script>

<div class="jconfirm" class:asking={st === 'asking'} role="group" aria-label="Yap asks before sending an agent" data-testid="chat-agent-confirm" data-state={st}>
  <svg class="jicon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" /></svg>
  <div class="jbody">
    <p class="jq">{q.lead} <strong>‘{q.quoted}’</strong>{q.then}</p>
    {#if path}<p class="jpath" title={path}>{path}</p>{/if}
    <dl class="jfacts">
      <dt>Mode</dt>
      <dd class:bypass>{j.modeLabel || 'Auto'}</dd>
    </dl>
    {#if brief}
      <p class="jlabel">What Claude Code would be asked</p>
      <!-- svelte-ignore a11y_no_noninteractive_tabindex (it scrolls: the keyboard reaches it) -->
      <div class="jbrief" tabindex="0" role="region" aria-label="The agent's brief" data-testid="chat-agent-brief">{brief}</div>
    {/if}
    {#if st === 'asking'}
      <p class="jwhy">{whyOf(card)} Only you can say yes.</p>
      <div class="jbtns">
        <button type="button" class="jstart" onclick={() => answer(true)}>Send</button>
        <button type="button" class="jdeny" onclick={() => answer(false)}>Don't</button>
      </div>
    {:else}
      <p class="jdone" aria-live="polite">{done(st)}</p>
    {/if}
  </div>
</div>

<style>
  .jconfirm {
    display: flex;
    gap: 10px;
    max-width: min(560px, 100%);
    margin: 6px 0 2px;
    padding: 10px 12px;
    border: 1px solid var(--yap-paper-rule);
    border-radius: 10px;
    background: var(--yap-paper-s1);
    color: var(--yap-paper-ink);
    font-size: 13px;
  }
  .jconfirm.asking {
    border-color: var(--yap-primary-line);
    background: var(--yap-paper);
    box-shadow: 0 0 0 3px var(--yap-primary-wash);
  }
  .jicon {
    flex: 0 0 auto;
    width: 18px;
    height: 18px;
    margin-top: 1px;
    color: var(--yap-primary);
  }
  .jbody {
    flex: 1 1 auto;
    min-width: 0;
  }
  .jq,
  .jpath,
  .jwhy,
  .jdone,
  .jlabel {
    margin: 0;
  }
  .jq strong {
    font-weight: 650;
    overflow-wrap: anywhere;
  }
  .jpath {
    margin-top: 1px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
    color: var(--yap-paper-muted);
  }
  .jfacts {
    display: flex;
    gap: 6px;
    margin: 6px 0 0;
    font-size: 12px;
  }
  .jfacts dt {
    color: var(--yap-paper-muted);
  }
  .jfacts dd {
    margin: 0;
    font-weight: 600;
  }
  .jfacts dd.bypass {
    color: var(--yap-warning);
  }
  .jlabel {
    margin-top: 8px;
    font-size: 11.5px;
    font-weight: 600;
    color: var(--yap-paper-text-2);
  }
  .jbrief {
    max-height: 160px;
    margin-top: 3px;
    padding: 8px 10px;
    overflow-y: auto;
    border: 1px solid var(--yap-paper-s2);
    border-radius: 8px;
    background: var(--yap-paper-s1);
    font-size: 12.5px;
    line-height: 1.5;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .jbrief:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  .jwhy,
  .jdone {
    margin-top: 8px;
    font-size: 12px;
    color: var(--yap-paper-muted);
  }
  .jbtns {
    display: flex;
    gap: 8px;
    margin-top: 9px;
  }
  .jbtns button {
    height: 28px;
    padding: 0 14px;
    border: 1px solid var(--yap-paper-rule);
    border-radius: 999px;
    background: var(--yap-paper);
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .jbtns .jstart {
    border-color: var(--yap-primary-line);
    background: var(--yap-primary);
    color: var(--yap-primary-fg);
  }
  .jbtns button:hover {
    background: var(--yap-paper-s2);
  }
  .jbtns .jstart:hover {
    background: var(--yap-primary-hover);
  }
  .jbtns button:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
</style>
