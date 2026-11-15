<script>
  // **A chat's Outputs, beside it** (15 Nov 2026; after the Claude app's Outputs list): every page it kept,
  // document it wrote, file its work changed and file it shared — the one list (outputs.js `outputItems`,
  // drawn by OutputsList.svelte, as a Code chat's summary draws it), each row opening where it lives
  // (outputsOpen.js). Any chat's: from the panel's row or the conversation's ⋯ → Outputs.
  import { agents as agentsStore } from '../../stores/agents.svelte.js';
  import { outputItems } from './outputs.js';
  import { openOutput } from './outputsOpen.js';
  import OutputsList from './OutputsList.svelte';

  /** `doc`: the panel; `messages`: the conversation's; `made`: what it made; `conversationId`: the chat. */
  let { doc, messages = [], made = [], conversationId = null } = $props();

  const chatAgents = $derived(conversationId == null ? [] : agentsStore.list.filter((a) => a.conversation === conversationId));
  const rows = $derived(outputItems({ agents: chatAgents, messages, made }));
</script>

<section class="outputs" data-testid="chat-outputs-tab" aria-label="Outputs">
  <h2 class="ohead">Outputs</h2>
  {#if rows.length}
    <OutputsList items={rows} max={40} onopen={(it) => openOutput(it, { doc, conversationId })} />
  {:else}
    <p class="oempty" data-testid="chat-outputs-empty">Nothing made yet. The pages this chat keeps, the documents it writes and the files it changes or shares show here.</p>
  {/if}
</section>

<style>
  .outputs {
    flex: 1 1 auto;
    min-height: 0;
    overflow: auto;
    padding: 16px 16px 24px;
  }
  .ohead {
    margin: 0 0 8px 6px;
    font-size: 13px;
    font-weight: 600;
    color: var(--yap-muted);
  }
  .oempty {
    margin: 4px 6px;
    color: var(--yap-muted);
    font-size: 13px;
    line-height: 1.5;
  }
</style>
