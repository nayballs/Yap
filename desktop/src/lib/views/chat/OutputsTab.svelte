<script>
  // **A chat's Outputs, beside it** (15 Nov 2026; after the Claude app's Outputs list): every page it kept,
  // document it wrote and file it shared (outputs.js), each a row — its icon, its name, what it is — that
  // opens it: a page or a document beside the chat, a picture in its tab, any other file in its app.
  import { invoke } from '@tauri-apps/api/core';
  import { toast } from '../../ui/toast.svelte.js';
  import { outputsOf } from './outputs.js';
  import { asShownPicture } from './shares.js';

  /** `doc`: the panel; `messages`: the conversation's; `made`: what it made; `conversationId`: the chat. */
  let { doc, messages = [], made = [], conversationId = null } = $props();

  const rows = $derived(outputsOf(messages, made));

  async function openRow(r) {
    if (r.kind === 'page') doc.showKept({ id: r.page.id, conversation: conversationId, title: r.page.title });
    else if (r.kind === 'doc') doc.show(r.id, { title: r.title });
    else if (r.item?.id && !r.item.gone) doc.showPicture(asShownPicture(r.item));
    else if (r.item?.path) {
      try {
        await invoke('chat_file_open', { conversation: conversationId, path: r.item.path });
      } catch (e) {
        toast({ title: `Couldn't open ${r.title}`, description: String(e), variant: 'destructive' });
      }
    }
  }
</script>

<section class="outputs" data-testid="chat-outputs" aria-label="Outputs">
  <h2 class="ohead">Outputs</h2>
  {#if rows.length}
    <ul class="olist">
      {#each rows as r (r.key)}
        <li>
          <button type="button" class="orow" onclick={() => openRow(r)} data-testid="chat-output" data-kind={r.kind} title={`Open ${r.title}`}>
            {#if r.kind === 'page'}
              <svg class="oicon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="8" y="3" width="13" height="13" rx="2" /><path d="M16 16v3a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-9a2 2 0 0 1 2-2h3" /></svg>
            {:else if r.kind === 'doc'}
              <svg class="oicon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z" /><path d="M14 3v5h5" /><path d="M9 13h6M9 17h4" /></svg>
            {:else if r.item?.kind === 'image'}
              <svg class="oicon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3" y="4" width="18" height="16" rx="2" /><circle cx="9" cy="10" r="2" /><path d="M21 16l-5-5-8 8" /></svg>
            {:else}
              <svg class="oicon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z" /><path d="M14 3v5h5" /></svg>
            {/if}
            <span class="oname">{r.title}</span>
            <span class="olabel">{r.label}</span>
          </button>
        </li>
      {/each}
    </ul>
  {:else}
    <p class="oempty" data-testid="chat-outputs-empty">Nothing made yet. The pages this chat keeps, the documents it writes and the files it shares show here.</p>
  {/if}
</section>

<style>
  .outputs {
    flex: 1 1 auto;
    min-height: 0;
    overflow: auto;
    padding: 16px 18px 24px;
  }
  .ohead {
    margin: 0 0 10px;
    font-size: 13px;
    font-weight: 600;
    color: var(--yap-muted);
  }
  .olist {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .orow {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 7px 8px;
    border: 0;
    border-radius: var(--yap-r-sm, 6px);
    background: transparent;
    color: var(--yap-fg);
    font: inherit;
    font-size: 13.5px;
    text-align: left;
    cursor: pointer;
  }
  .orow:hover {
    background: var(--yap-s3);
  }
  .orow:focus-visible {
    outline: 2px solid var(--yap-focus-ring, var(--yap-primary));
    outline-offset: -2px;
  }
  .oicon {
    flex: none;
    width: 16px;
    height: 16px;
    color: var(--yap-muted);
  }
  .oname {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .olabel {
    flex: none;
    color: var(--yap-muted);
    font-size: 12.5px;
  }
  .oempty {
    margin: 4px 0;
    color: var(--yap-muted);
    font-size: 13px;
    line-height: 1.5;
  }
</style>
