<script>
  // **Yap asks before acting on a page** (chat/pages/act/confirm.rs): a
  // click (or Enter, or a link go_to would open) that would buy, pay, book,
  // confirm, order, cancel, subscribe, send, delete or sign in — or typing
  // words that aren't in what the person said nor on the page — waits for
  // the person: "Yap wants to press ‘Book now’ on rail.example.com", Allow
  // once / Don't. The same question shows in its page tab's strip
  // (PageTab.svelte: the panel may cover the thread); either answers it. No
  // answer in two minutes is a no; each OK is for that one action. The
  // answer keeps its cards (ChatMessage.confirms), answered. Its own small
  // chunk. An agent about to start (act "agent") has a card of its own,
  // AgentConfirm.svelte, and so does a connector's tool (act "connector"),
  // ConnectorConfirm.svelte.
  import { invoke } from '@tauri-apps/api/core';
  import { questionParts, whyOf, doneWords } from './pageConfirm.js';
  import AgentConfirm from './AgentConfirm.svelte';
  import ConnectorConfirm from './ConnectorConfirm.svelte';

  /** `confirms`: the turn's questions (`confirm` events: id, label, site, does, state, act, text). */
  let { confirms = [] } = $props();

  // Answered here, before Rust's next event says so.
  let answered = $state({});

  const stateOf = (c) => (c.state === 'asking' && answered[c.id]) || c.state;

  async function answer(c, allow) {
    answered = { ...answered, [c.id]: allow ? 'allowed' : 'refused' };
    try {
      await invoke('chat_page_confirm', { id: c.id, allow });
    } catch {
      /* the question had lapsed (the turn stopped): its card says no */
    }
  }
</script>

{#each confirms as c (c.id)}
  {@const st = stateOf(c)}
  {@const q = questionParts(c)}
  {#if c.act === 'agent'}
    <AgentConfirm card={c} />
  {:else if c.act === 'connector'}
    <ConnectorConfirm card={c} />
  {:else}
  <div class="pconfirm" class:asking={st === 'asking'} role="group" aria-label="Yap asks before acting on the page" data-testid="chat-page-confirm" data-state={st}>
    <svg class="picon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3l8 3v6c0 4.5-3.4 8.2-8 9-4.6-.8-8-4.5-8-9V6z" /><path d="M12 8v5M12 16.5v.01" /></svg>
    <div class="pbody">
      <p class="pq">{q.lead} <strong>‘{q.quoted}’</strong>{q.then}{#if c.site}{' '}on <span class="psite">{c.site}</span>{/if}</p>
      {#if st === 'asking'}
        <p class="pwhy">{whyOf(c)} Only you can say yes.</p>
        <div class="pbtns">
          <button type="button" class="pallow" onclick={() => answer(c, true)}>Allow once</button>
          <button type="button" class="pdeny" onclick={() => answer(c, false)}>Don't</button>
        </div>
      {:else}
        <p class="pdone" aria-live="polite">{doneWords(c, st)}</p>
      {/if}
    </div>
  </div>
  {/if}
{/each}

<style>
  .pconfirm {
    display: flex;
    gap: 10px;
    max-width: min(520px, 100%);
    margin: 6px 0 2px;
    padding: 10px 12px;
    border: 1px solid var(--yap-paper-rule);
    border-radius: 10px;
    background: var(--yap-paper-s1);
    color: var(--yap-paper-ink);
    font-size: 13px;
  }
  .pconfirm.asking {
    border-color: var(--yap-primary-line);
    background: var(--yap-paper);
    box-shadow: 0 0 0 3px var(--yap-primary-wash);
  }
  .picon {
    flex: 0 0 auto;
    width: 18px;
    height: 18px;
    margin-top: 1px;
    color: var(--yap-paper-muted);
  }
  .pbody {
    min-width: 0;
  }
  .pq,
  .pwhy,
  .pdone {
    margin: 0;
  }
  .pq strong {
    font-weight: 650;
    overflow-wrap: anywhere;
  }
  .psite {
    color: var(--yap-paper-text-2);
  }
  .pwhy,
  .pdone {
    margin-top: 3px;
    font-size: 12px;
    color: var(--yap-paper-muted);
  }
  .pbtns {
    display: flex;
    gap: 8px;
    margin-top: 9px;
  }
  .pbtns button {
    height: 28px;
    padding: 0 12px;
    border: 1px solid var(--yap-paper-rule);
    border-radius: 999px;
    background: var(--yap-paper);
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .pbtns .pallow {
    border-color: var(--yap-primary-line);
    background: var(--yap-primary);
    color: var(--yap-primary-fg);
  }
  .pbtns button:hover {
    background: var(--yap-paper-s2);
  }
  .pbtns .pallow:hover {
    background: var(--yap-primary-hover);
  }
  .pbtns button:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
</style>
