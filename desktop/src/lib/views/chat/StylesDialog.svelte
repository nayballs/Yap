<script>
  // **Create & edit styles** (chat/styles.rs, 15 Nov 2026; the Claude app's
  // "Create & edit styles"): Yap's four, the person's own (Use, Edit,
  // Delete…), and a new one — described in a sentence or two, or made from a
  // sample of their writing: Chat's model writes how to write (never the
  // sample's topic), the person reads and edits it, then saves. The sample is
  // never kept. Esc, the scrim and Done close it. Its own chunk.
  import { invoke } from '@tauri-apps/api/core';
  import { tick, onMount } from 'svelte';
  import ConfirmDialog from '../../ui/ConfirmDialog.svelte';
  import { toast } from '../../ui/toast.svelte.js';

  /** `onclose()`; `onpick(id)`: Use a style in the chat shown. */
  let { onclose, onpick = () => {} } = $props();

  let card = $state(null);
  let builtins = $state([]);
  let mine = $state([]);
  // The one being made or edited: `{ id, name, instructions, from }`, or null.
  let draft = $state(null);
  // How a new one is made: 'description' | 'sample'.
  let way = $state('description');
  let source = $state('');
  let writing = $state(false);
  let saving = $state(false);
  let askDelete = $state(null);
  let deleteOpen = $state(false);
  let deleting = $state(false);

  async function load() {
    try {
      const out = await invoke('chat_styles');
      builtins = out?.builtins || [];
      mine = out?.styles || [];
    } catch (e) {
      toast({ title: "Couldn't read your styles", description: String(e), variant: 'destructive' });
    }
  }
  onMount(() => {
    load();
    tick().then(() => card?.querySelector('[data-first]')?.focus({ preventScroll: true }));
  });

  function startNew() {
    draft = { id: '', name: '', instructions: '', from: '' };
    way = 'description';
    source = '';
    tick().then(() => card?.querySelector('[data-source]')?.focus());
  }
  function startEdit(s) {
    draft = { id: s.id, name: s.name, instructions: s.instructions, from: s.from || '' };
    source = '';
    tick().then(() => card?.querySelector('[data-name]')?.focus());
  }

  async function write() {
    if (!source.trim() || writing) return;
    writing = true;
    try {
      const out = await invoke('chat_style_write', way === 'sample' ? { sample: source } : { description: source });
      draft = { ...draft, name: draft.name.trim() || out?.name || '', instructions: out?.instructions || '', from: way };
      tick().then(() => card?.querySelector('[data-instructions]')?.focus());
    } catch (e) {
      toast({ title: "Couldn't write the style", description: String(e), variant: 'destructive' });
    } finally {
      writing = false;
    }
  }

  async function save() {
    if (!draft || saving) return;
    saving = true;
    try {
      const saved = await invoke('chat_style_save', { style: draft });
      draft = null;
      source = '';
      await load();
      toast({ title: `Style “${saved.name}” saved`, variant: 'success' });
    } catch (e) {
      toast({ title: "Couldn't save the style", description: String(e), variant: 'destructive' });
    } finally {
      saving = false;
    }
  }

  async function doDelete() {
    if (!askDelete) return;
    deleting = true;
    try {
      await invoke('chat_style_delete', { id: askDelete.id });
      if (draft?.id === askDelete.id) draft = null;
      deleteOpen = false;
      askDelete = null;
      await load();
    } catch (e) {
      toast({ title: "Couldn't delete the style", description: String(e), variant: 'destructive' });
    } finally {
      deleting = false;
    }
  }

  function onKeydown(e) {
    if (e.key === 'Escape' && !deleteOpen) {
      e.preventDefault();
      e.stopPropagation();
      if (draft) draft = null;
      else onclose?.();
    }
  }
</script>

<div class="scrim" role="presentation" onclick={(e) => e.target === e.currentTarget && onclose?.()}>
  <div bind:this={card} class="dialog" role="dialog" aria-modal="true" aria-labelledby="chat-styles-title" tabindex="-1" onkeydown={onKeydown} data-testid="chat-styles-dialog">
    <div class="head">
      <h2 id="chat-styles-title">Styles</h2>
      <p class="sub">How answers are written. Pick one for a chat from Style under the message box; a new chat starts with the last one you picked.</p>
    </div>

    <div class="body">
      <ul class="list" aria-label="Yap's styles">
        {#each builtins as s (s.id)}
          <li class="row">
            <div class="rtext">
              <span class="rname">{s.name}</span>
              <span class="rabout">{s.about}</span>
            </div>
            <button type="button" class="link" data-first={s.id === 'normal' ? '' : undefined} onclick={() => onpick(s.id)}>Use</button>
          </li>
        {/each}
      </ul>

      <h3>Your styles</h3>
      {#if mine.length}
        <ul class="list" aria-label="Your styles">
          {#each mine as s (s.id)}
            <li class="row">
              <div class="rtext">
                <span class="rname">{s.name}</span>
                <span class="rabout">{s.instructions}</span>
              </div>
              <button type="button" class="link" onclick={() => onpick(s.id)}>Use</button>
              <button type="button" class="link" onclick={() => startEdit(s)} aria-label={`Edit ${s.name}`}>Edit</button>
              <button type="button" class="link danger" onclick={() => ((askDelete = s), (deleteOpen = true))} aria-label={`Delete ${s.name}`}>Delete…</button>
            </li>
          {/each}
        </ul>
      {:else if !draft}
        <p class="none">None yet. Describe a style, or let Yap learn one from your own writing.</p>
      {/if}

      {#if draft}
        <form class="draft" aria-label={draft.id ? 'Edit style' : 'New style'} onsubmit={(e) => (e.preventDefault(), save())}>
          {#if !draft.id}
            <div class="ways" role="radiogroup" aria-label="Make it from">
              <label><input type="radio" name="way" value="description" bind:group={way} /> Describe it</label>
              <label><input type="radio" name="way" value="sample" bind:group={way} /> From your writing</label>
            </div>
            <textarea
              data-source
              rows={way === 'sample' ? 6 : 3}
              bind:value={source}
              aria-label={way === 'sample' ? 'A sample of your writing' : 'The style you want'}
              placeholder={way === 'sample' ? 'Paste something you wrote: an email, a post, a few paragraphs. Only how it reads is used, never what it says.' : 'For example: warm and brief, like a note to a friend, no jargon.'}
            ></textarea>
            <div class="actions">
              <button type="button" class="btn" disabled={!source.trim() || writing} onclick={write}>{writing ? 'Writing…' : 'Write the style'}</button>
            </div>
          {/if}
          <label class="field">
            <span>Name</span>
            <input data-name type="text" maxlength="40" bind:value={draft.name} placeholder="Like a friend" />
          </label>
          <label class="field">
            <span>How to write</span>
            <textarea data-instructions rows="4" maxlength="1200" bind:value={draft.instructions} placeholder="Write warmly and briefly, in plain words."></textarea>
          </label>
          <div class="actions">
            <button type="button" class="btn quiet" onclick={() => (draft = null)}>Cancel</button>
            <button type="submit" class="btn primary" disabled={saving || !draft.name.trim() || !draft.instructions.trim()}>{saving ? 'Saving…' : 'Save style'}</button>
          </div>
        </form>
      {/if}
    </div>

    <div class="foot">
      {#if !draft}<button type="button" class="btn" onclick={startNew}>Create a style</button>{/if}
      <span class="gap"></span>
      <button type="button" class="btn" onclick={() => onclose?.()}>Done</button>
    </div>
  </div>
</div>

<ConfirmDialog
  bind:open={deleteOpen}
  title={askDelete ? `Delete “${askDelete.name}”?` : ''}
  body="Chats that use it go back to Normal."
  busy={deleting}
  onconfirm={doDelete}
/>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 95;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 16px;
    background: var(--yap-paper-scrim);
  }
  .dialog {
    display: flex;
    flex-direction: column;
    gap: 14px;
    width: min(560px, 100%);
    max-height: min(680px, 88vh);
    box-sizing: border-box;
    padding: 22px 24px 18px;
    border: 1px solid var(--yap-paper-s2);
    border-radius: 14px;
    background: var(--yap-paper);
    color: var(--yap-paper-ink);
    box-shadow: var(--yap-shadow-dialog);
    outline: none;
  }
  h2 {
    margin: 0 0 6px;
    font-size: 17px;
    font-weight: 600;
    line-height: 24px;
  }
  h3 {
    margin: 14px 0 6px;
    font-size: 13px;
    font-weight: 600;
  }
  .sub,
  .none {
    margin: 0;
    font-size: 13px;
    line-height: 19px;
    color: var(--yap-paper-text-2);
  }
  .body {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
  }
  .list {
    margin: 0;
    padding: 0;
    list-style: none;
    border: 1px solid var(--yap-paper-s2);
    border-radius: 10px;
    background: var(--yap-paper-card);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 12px;
  }
  .row + .row {
    border-top: 1px solid var(--yap-paper-s2);
  }
  .rtext {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .rname {
    font-size: 13px;
    font-weight: 600;
  }
  .rabout {
    overflow: hidden;
    font-size: 12px;
    color: var(--yap-paper-text-2);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .link {
    flex: none;
    padding: 2px 4px;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--yap-primary);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .link.danger {
    color: var(--yap-danger);
  }
  .link:hover {
    text-decoration: underline;
  }
  .link:focus-visible,
  .btn:focus-visible {
    outline: none;
    box-shadow: 0 0 0 3px var(--yap-paper-ring);
  }
  .draft {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin-top: 12px;
    padding: 12px;
    border: 1px solid var(--yap-paper-s2);
    border-radius: 10px;
    background: var(--yap-paper-card);
  }
  .ways {
    display: flex;
    gap: 16px;
    font-size: 13px;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 12px;
    font-weight: 600;
    color: var(--yap-paper-text-2);
  }
  input[type='text'],
  textarea {
    box-sizing: border-box;
    width: 100%;
    padding: 7px 9px;
    border: 1px solid var(--yap-paper-s2);
    border-radius: 8px;
    background: var(--yap-paper);
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 13px;
    font-weight: 400;
    resize: vertical;
  }
  input[type='text']:focus,
  textarea:focus {
    outline: none;
    border-color: var(--yap-paper-s2-strong);
    box-shadow: 0 0 0 3px var(--yap-paper-ring);
  }
  .actions,
  .foot {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
  }
  .gap {
    flex: 1 1 auto;
  }
  .btn {
    height: 32px;
    padding: 0 14px;
    border: 1px solid var(--yap-paper-s2);
    border-radius: 9999px;
    background: var(--yap-paper-card);
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
  }
  .btn:hover:not(:disabled) {
    border-color: var(--yap-paper-s2-strong);
  }
  .btn:disabled {
    opacity: 0.55;
    cursor: default;
  }
  .btn.primary {
    border-color: var(--yap-primary);
    background: var(--yap-primary);
    color: var(--yap-primary-fg);
  }
  .btn.quiet {
    border-color: transparent;
    background: none;
  }
</style>
