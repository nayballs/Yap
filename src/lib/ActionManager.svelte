<script>
  // Actions manager dialog — port of OpenWhispr's ActionManagerDialog.tsx:
  // a two-pane modal over the note Actions (named prompt fragments). Left:
  // action list ("Built-in" badge, hover-trash for custom, + to add). Right:
  // Name / Description / Prompt editor with Save/Update. Built-ins can be
  // edited but not deleted.
  import { invoke } from '@tauri-apps/api/core';
  import { toast } from './ui/toast.svelte.js';

  let { open = $bindable(false), onchanged = null } = $props();

  let actions = $state([]);
  let selectedId = $state(null);
  let creating = $state(false);
  let name = $state('');
  let description = $state('');
  let prompt = $state('');
  let saving = $state(false);
  let error = $state(null);
  let nameEl = $state(null);

  const selected = $derived(actions.find((a) => a.id === selectedId) || null);
  const dirty = $derived.by(() => {
    if (creating) return name.trim() !== '' || prompt.trim() !== '';
    if (!selected) return false;
    return (
      name !== selected.name || description !== selected.description || prompt !== selected.prompt
    );
  });

  async function refresh() {
    try {
      actions = (await invoke('notes_actions')) || [];
    } catch {
      actions = [];
    }
  }

  // (Re)initialize whenever the dialog opens: load + select the first action.
  $effect(() => {
    if (!open) return;
    error = null;
    creating = false;
    refresh().then(() => {
      if (actions.length > 0) selectAction(actions[0]);
      else startCreate();
    });
  });

  function selectAction(a) {
    selectedId = a.id;
    creating = false;
    name = a.name;
    description = a.description || '';
    prompt = a.prompt;
    error = null;
  }

  function startCreate() {
    selectedId = null;
    creating = true;
    name = '';
    description = '';
    prompt = '';
    error = null;
    setTimeout(() => nameEl?.focus(), 50);
  }

  async function save() {
    if (!name.trim() || !prompt.trim()) return;
    saving = true;
    error = null;
    try {
      if (creating) {
        const a = await invoke('action_create', {
          name: name.trim(),
          description: description.trim(),
          prompt: prompt.trim(),
        });
        await refresh();
        selectAction(actions.find((x) => x.id === a.id) || a);
        toast({ title: 'Action created', description: a.name, variant: 'success' });
      } else if (selected) {
        await invoke('action_update', {
          id: selected.id,
          name: name.trim(),
          description: description.trim(),
          prompt: prompt.trim(),
        });
        await refresh();
        const again = actions.find((x) => x.id === selectedId);
        if (again) selectAction(again);
        toast({ title: 'Action updated', variant: 'success' });
      }
      onchanged?.();
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }

  async function remove(id) {
    try {
      await invoke('action_delete', { id });
      await refresh();
      if (selectedId === id) {
        if (actions.length > 0) selectAction(actions[0]);
        else startCreate();
      }
      onchanged?.();
    } catch (e) {
      error = String(e);
    }
  }

  function close() {
    open = false;
  }
</script>

{#if open}
  <div
    class="backdrop"
    role="presentation"
    onclick={(e) => e.target === e.currentTarget && close()}
  >
    <div class="dialog" role="dialog" aria-label="Manage actions">
      <button class="x" aria-label="Close" onclick={close}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
      </button>

      <!-- Left: action list -->
      <div class="pane list">
        <div class="listhead">
          <span>Manage actions</span>
          <button class="add" title="Add action" aria-label="Add action" onclick={startCreate}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
          </button>
        </div>
        <div class="items">
          {#each actions as a (a.id)}
            <div
              class="item"
              class:active={selectedId === a.id && !creating}
              role="button"
              tabindex="0"
              onclick={() => selectAction(a)}
              onkeydown={(e) => (e.key === 'Enter' || e.key === ' ') && selectAction(a)}
            >
              <svg class="spark" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3v3M12 18v3M3 12h3M18 12h3M5.6 5.6l2.1 2.1M16.3 16.3l2.1 2.1M18.4 5.6l-2.1 2.1M7.7 16.3l-2.1 2.1" /></svg>
              <span class="iname">{a.name}</span>
              {#if a.builtin}
                <span class="badge">Built-in</span>
              {:else}
                <button
                  class="del"
                  title="Delete action"
                  aria-label="Delete action"
                  onclick={(e) => {
                    e.stopPropagation();
                    remove(a.id);
                  }}
                >
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 6h18M8 6V4a1 1 0 0 1 1-1h6a1 1 0 0 1 1 1v2m3 0v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6" /></svg>
                </button>
              {/if}
            </div>
          {/each}
        </div>
      </div>

      <!-- Right: editor -->
      <div class="pane editor">
        <div class="edhead">
          <span>{creating ? 'Add action' : 'Edit action'}</span>
          <div class="edbtns">
            {#if creating && actions.length > 0}
              <button class="ghost" disabled={saving} onclick={() => selectAction(actions[0])}>Cancel</button>
            {/if}
            <button
              class="save"
              disabled={saving || !name.trim() || !prompt.trim() || !dirty}
              onclick={save}
            >
              {saving ? 'Saving…' : creating ? 'Save' : 'Update'}
            </button>
          </div>
        </div>
        <div class="form">
          <input bind:this={nameEl} bind:value={name} placeholder="Action name" disabled={saving} />
          <input bind:value={description} placeholder="Short description (optional)" disabled={saving} />
          <label class="plabel" for="action-prompt">Prompt</label>
          <textarea
            id="action-prompt"
            bind:value={prompt}
            disabled={saving}
            placeholder="What should this action do with the note? e.g. “Summarize into 5 bullet points with a one-line takeaway.”"
          ></textarea>
          {#if error}<p class="err">{error}</p>{/if}
          <p class="hint">
            Runs under Yap's note guardrails (clean markdown, no preamble) with the Note
            Formatting model.
          </p>
        </div>
      </div>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 60;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(45, 38, 25, 0.35);
  }
  .dialog {
    position: relative;
    display: flex;
    width: min(760px, calc(100vw - 60px));
    height: min(480px, calc(100vh - 80px));
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r-xl);
    overflow: hidden;
    background: var(--yap-s1);
    box-shadow: var(--yap-shadow-modal);
  }
  .x {
    position: absolute;
    top: 10px;
    right: 12px;
    z-index: 5;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    border: none;
    border-radius: var(--yap-r-sm);
    background: none;
    color: var(--yap-muted-55);
    cursor: pointer;
  }
  .x:hover {
    background: var(--yap-s2);
    color: var(--yap-fg);
  }
  .x svg {
    width: 13px;
    height: 13px;
  }

  .pane.list {
    flex: 0 0 220px;
    display: flex;
    flex-direction: column;
    border-right: 1px solid var(--yap-border-subtle);
    background: var(--yap-s0, var(--yap-s1));
  }
  .listhead {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px 12px 8px;
    font-size: 12px;
    font-weight: 600;
  }
  .add {
    display: inline-flex;
    width: 20px;
    height: 20px;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: var(--yap-r-sm);
    background: none;
    color: var(--yap-muted-55);
    cursor: pointer;
  }
  .add:hover {
    background: var(--yap-s2);
    color: var(--yap-fg);
  }
  .add svg {
    width: 11px;
    height: 11px;
  }
  .items {
    flex: 1 1 auto;
    overflow-y: auto;
    padding: 0 6px 10px;
  }
  .item {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 9px;
    border-radius: var(--yap-r);
    cursor: pointer;
  }
  .item:hover {
    background: var(--yap-s2);
  }
  .item.active {
    background: var(--yap-primary-wash);
  }
  .spark {
    width: 12px;
    height: 12px;
    color: var(--yap-muted-55);
    flex: 0 0 auto;
  }
  .item.active .spark {
    color: var(--yap-primary);
  }
  .iname {
    flex: 1 1 auto;
    min-width: 0;
    font-size: 12px;
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .badge {
    flex: 0 0 auto;
    font-size: 9px;
    font-weight: 600;
    padding: 1px 5px;
    border-radius: 4px;
    background: var(--yap-s2);
    color: var(--yap-muted-55);
  }
  .del {
    display: inline-flex;
    width: 20px;
    height: 20px;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: var(--yap-r-sm);
    background: none;
    color: var(--yap-muted-55);
    cursor: pointer;
    opacity: 0;
    flex: 0 0 auto;
  }
  .item:hover .del {
    opacity: 1;
  }
  .del:hover {
    color: #ef4444;
  }
  .del svg {
    width: 11px;
    height: 11px;
  }

  .pane.editor {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .edhead {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px 44px 10px 18px;
    border-bottom: 1px solid var(--yap-border-subtle);
    font-size: 11.5px;
    color: var(--yap-muted);
  }
  .edbtns {
    display: flex;
    gap: 8px;
  }
  .ghost {
    height: 27px;
    padding: 0 10px;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-muted);
    font: inherit;
    font-size: 11.5px;
    cursor: pointer;
  }
  .ghost:hover {
    background: var(--yap-s2);
    color: var(--yap-fg);
  }
  .save {
    height: 27px;
    padding: 0 13px;
    border: none;
    border-radius: var(--yap-r);
    background: var(--yap-ink, var(--yap-primary));
    color: var(--yap-ink-fg, var(--yap-primary-fg));
    font: inherit;
    font-size: 11.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .save:hover:not(:disabled) {
    background: var(--yap-ink-hover, var(--yap-primary-hover));
  }
  .save:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .form {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 14px 18px 16px;
    min-height: 0;
  }
  .form input {
    height: 32px;
    padding: 0 11px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
    color: var(--yap-fg);
    font: inherit;
    font-size: 12.5px;
  }
  .form input:focus,
  .form textarea:focus {
    outline: none;
    border-color: var(--yap-primary);
  }
  .plabel {
    font-size: 11px;
    font-weight: 600;
    color: var(--yap-muted);
    margin-top: 2px;
  }
  .form textarea {
    flex: 1 1 auto;
    min-height: 120px;
    resize: none;
    padding: 10px 12px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
    color: var(--yap-fg);
    font-family: ui-monospace, Consolas, monospace;
    font-size: 12px;
    line-height: 1.6;
  }
  .err {
    margin: 0;
    font-size: 11.5px;
    color: #ef4444;
  }
  .hint {
    margin: 0;
    font-size: 10.5px;
    color: var(--yap-muted-55);
  }
</style>
