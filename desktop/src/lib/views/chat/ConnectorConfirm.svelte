<script>
  // **Yap asks before a connector's tool** (chat/tools/connectors.rs, a
  // `confirm` card with act "connector"; chat/pages/act/confirm.rs): a tool
  // set to Ask first, one that can delete, a write after the chat read
  // something from outside, a name that sends, pays or unlocks, or a
  // meeting on. It reads as the tool, not a page: "Use Notion: Create
  // page?", the arguments as key: value lines (secrets masked, long values
  // cut by Rust), why Yap asks, and **Allow once · Always allow · Deny**
  // (`chat_connector_confirm`; no Always allow for a tool that can delete,
  // nor one that's Always allow already). Asked out loud, Yap also says it
  // and the Yap bar asks (assistant/spoken_ok.rs): a spoken yes or no,
  // or the bar's Allow once / Don't, answers this same card — unless it
  // needs a click (it can delete, a meeting is on). No answer in two
  // minutes is a no. Kept with the answer, answered
  // (ChatMessage.confirms). In PageConfirm's chunk.
  import { invoke } from '@tauri-apps/api/core';

  /** `card`: the confirm event (`{id, state, connector: {connector, tool, args: [{key, value, cut, secret}], always, why, kind, voice, spoken}}`). */
  let { card } = $props();

  // Answered here, before Rust's next event says so.
  let answered = $state('');
  const st = $derived((card.state === 'asking' && answered) || card.state);
  const c = $derived(card.connector ?? {});
  const name = $derived(c.connector || card.site || 'a connector');
  const tool = $derived(c.tool || card.label || 'a tool');
  const args = $derived(Array.isArray(c.args) ? c.args : []);

  const WHY = {
    ask: 'You set this tool to ask first.',
    delete: 'It can delete things, so Yap always asks.',
    outside: 'This chat read something from outside Yap, which could have asked for this.',
    risky: 'It may send, pay for or unlock something.',
    meeting: 'A meeting is on, so Yap asks before every change.',
  };
  const why = $derived(WHY[c.why] || 'It may change something.');

  async function answer(a) {
    answered = a === 'always' ? 'always' : a === 'once' ? 'allowed' : 'refused';
    try {
      await invoke('chat_connector_confirm', { id: card.id, answer: a });
    } catch {
      /* the question had lapsed (the turn stopped): its card says no */
    }
  }

  const done = (s) =>
    s === 'always'
      ? `Always allowed: Yap won't ask about ${tool} again.`
      : s === 'allowed'
        ? 'Allowed once.'
        : s === 'timedOut'
          ? "No answer in time, so Yap didn't use it."
          : "Yap didn't use it.";
</script>

<div class="cconfirm" class:asking={st === 'asking'} role="group" aria-label="Yap asks before using a connector" data-testid="chat-connector-confirm" data-state={st}>
  <svg class="cicon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"
    ><path d="M9 7V3M15 7V3" /><path d="M6 7h12v4a6 6 0 0 1-12 0z" /><path d="M12 17v4" /></svg
  >
  <div class="cbody">
    <p class="cq">Use <strong>{name}</strong>: <strong>{tool}</strong>?</p>
    {#if args.length}
      <!-- svelte-ignore a11y_no_noninteractive_tabindex (it scrolls: the keyboard reaches it) -->
      <dl class="cargs" tabindex="0" aria-label="What it would send" data-testid="chat-connector-args">
        {#each args as a, i (i)}
          <dt>{a.key}</dt>
          <dd class:secret={a.secret} class:cut={a.cut} title={a.secret ? 'Hidden: it looks like a secret' : undefined}>{a.value}</dd>
        {/each}
      </dl>
    {/if}
    {#if st === 'asking'}
      <!-- Asked out loud (assistant/spoken_ok.rs): a spoken yes or no answers it too, unless it needs a click. -->
      <p class="cwhy">{why}{#if c.voice}{' '}{c.spoken ? 'You asked out loud: say yes or no, or answer here.' : 'You asked out loud: this one needs a click.'}{/if} Only you can say yes.</p>
      <div class="cbtns">
        <button type="button" class="callow" onclick={() => answer('once')}>Allow once</button>
        {#if c.always}<button type="button" class="calways" onclick={() => answer('always')}>Always allow</button>{/if}
        <button type="button" class="cdeny" onclick={() => answer('deny')}>Deny</button>
      </div>
    {:else}
      <p class="cdone" aria-live="polite">{done(st)}</p>
    {/if}
  </div>
</div>

<style>
  .cconfirm {
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
  .cconfirm.asking {
    border-color: var(--yap-primary-line);
    background: var(--yap-paper);
    box-shadow: 0 0 0 3px var(--yap-primary-wash);
  }
  .cicon {
    flex: 0 0 auto;
    width: 18px;
    height: 18px;
    margin-top: 1px;
    color: var(--yap-primary);
  }
  .cbody {
    flex: 1 1 auto;
    min-width: 0;
  }
  .cq,
  .cwhy,
  .cdone {
    margin: 0;
  }
  .cq strong {
    font-weight: 650;
    overflow-wrap: anywhere;
  }
  .cargs {
    display: grid;
    grid-template-columns: minmax(0, max-content) minmax(0, 1fr);
    column-gap: 10px;
    row-gap: 3px;
    max-height: 168px;
    margin: 7px 0 0;
    padding: 7px 10px;
    overflow-y: auto;
    border: 1px solid var(--yap-paper-s2);
    border-radius: 8px;
    background: var(--yap-paper-s1);
    font-size: 12.5px;
    line-height: 1.45;
  }
  .cargs:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  .cargs dt {
    max-width: 14em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--yap-paper-muted);
  }
  .cargs dd {
    margin: 0;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .cargs dd.secret {
    letter-spacing: 0.12em;
    color: var(--yap-paper-muted);
  }
  .cwhy,
  .cdone {
    margin-top: 8px;
    font-size: 12px;
    color: var(--yap-paper-muted);
  }
  .cbtns {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 9px;
  }
  .cbtns button {
    height: 28px;
    padding: 0 13px;
    border: 1px solid var(--yap-paper-rule);
    border-radius: 999px;
    background: var(--yap-paper);
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .cbtns .callow {
    border-color: var(--yap-primary-line);
    background: var(--yap-primary);
    color: var(--yap-primary-fg);
  }
  .cbtns button:hover {
    background: var(--yap-paper-s2);
  }
  .cbtns .callow:hover {
    background: var(--yap-primary-hover);
  }
  .cbtns button:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
</style>
