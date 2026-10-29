<script>
  // Settings → Commands → Writing samples (ai/writing_samples.rs; Wispr
  // Flow's "make it sound like you"): a few things you wrote yourself, so
  // rewrites sound like you — the rewrite shortcut, Commands, Polish,
  // Shorter and your own transforms (never AI cleanup, which keeps your
  // words). Each one tagged with the kind of app it suits (any app, email,
  // chat, documents), added, edited and deleted here (deleted behind Yap's
  // confirm dialog). They live in their own file, writing_samples.json,
  // never config.json: this page saves them itself (`writing_samples_set`),
  // not Settings' auto-save.
  import { tick } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import Select from '../ui/Select.svelte';
  import Textarea from '../ui/Textarea.svelte';
  import Button from '../ui/Button.svelte';
  import ConfirmDialog from '../ui/ConfirmDialog.svelte';
  import { showSaveError } from '../notices/saveErrors.js';

  const KINDS = [
    { value: '', label: 'Any app' },
    { value: 'email', label: 'Email' },
    { value: 'chat', label: 'Chat' },
    { value: 'documents', label: 'Documents' },
  ];
  const kindName = (k) => KINDS.find((x) => x.value === k)?.label || 'Any app';

  let samples = $state([]);
  let limits = $state({ max: 12, maxChars: 1500, budgetLocal: 800, budgetCloud: 2000 });
  let loaded = $state(false);
  let error = $state('');

  function take(s) {
    if (!s) return;
    samples = Array.isArray(s.samples) ? s.samples : [];
    limits = { max: s.max ?? 12, maxChars: s.maxChars ?? 1500, budgetLocal: s.budgetLocal ?? 800, budgetCloud: s.budgetCloud ?? 2000 };
    loaded = true;
  }

  invoke('writing_samples_get')
    .then(take)
    .catch(() => (loaded = true));

  async function save(next) {
    error = '';
    // What the page shows changes at once; Rust's answer gives new ones their id.
    samples = next;
    try {
      take(await invoke('writing_samples_set', { samples: next.map(({ id, text, kind }) => ({ id: id || '', text, kind: kind || '' })) }));
    } catch (e) {
      if (!showSaveError(e)) error = `Couldn't keep your writing samples: ${e}`;
    }
  }

  // The form: one sample being edited (its id), or 'new'; null: none.
  let editing = $state(null);
  let draftText = $state('');
  let draftKind = $state('');
  let formEl = $state(null);
  let addEl = $state(null);
  let deleting = $state(null);
  let confirmOpen = $state(false);

  async function openForm(s) {
    editing = s ? s.id : 'new';
    draftText = s ? s.text : '';
    draftKind = s ? s.kind || '' : '';
    await tick();
    formEl?.querySelector('textarea')?.focus();
  }

  function closeForm() {
    const was = editing;
    editing = null;
    tick().then(() => {
      const back = was === 'new' ? addEl : document.querySelector(`[data-sample="${was}"] .edit`);
      back?.focus?.({ preventScroll: true });
    });
  }

  const canSave = $derived(draftText.trim().length > 0);

  function saveForm() {
    if (!canSave) return;
    const text = draftText.trim();
    if (editing === 'new') {
      save([...samples, { id: '', text, kind: draftKind }]);
    } else {
      save(samples.map((s) => (s.id === editing ? { ...s, text, kind: draftKind } : s)));
    }
    closeForm();
  }

  function onFormKey(e) {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      closeForm();
    } else if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      saveForm();
    }
  }

  function setKind(s, kind) {
    save(samples.map((x) => (x.id === s.id ? { ...x, kind } : x)));
  }

  function askDelete(s) {
    deleting = s;
    confirmOpen = true;
  }

  function doDelete() {
    if (deleting) save(samples.filter((x) => x.id !== deleting.id));
    confirmOpen = false;
    deleting = null;
    tick().then(() => addEl?.focus?.({ preventScroll: true }));
  }

  const preview = (t) => (t || '').replace(/\s+/g, ' ').trim();
  // About four characters a token.
  const tokens = (chars) => Math.round(chars / 4 / 50) * 50;
</script>

<div class="samples" data-setting="writing-samples">
  {#each samples as s, i (s.id || `new-${i}`)}
    {#if editing === s.id}
      {@render form('Save')}
    {:else}
      <div class="sample" data-sample={s.id}>
        <p class="text" title={s.text}>{preview(s.text)}</p>
        <div class="rhs">
          <Select value={s.kind || ''} options={KINDS} ariaLabel="Kind of app it suits" onchange={(v) => setKind(s, v)} />
          <button class="tool edit" title="Edit" aria-label={`Edit writing sample ${i + 1}`} onclick={() => openForm(s)}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 20h4L19 9l-4-4L4 16v4z" /><path d="m13.5 6.5 4 4" /></svg>
          </button>
          <button class="tool del" title="Delete" aria-label={`Delete writing sample ${i + 1}`} onclick={() => askDelete(s)}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 7h14M10 7V5h4v2M7 7l1 12h8l1-12" /></svg>
          </button>
        </div>
      </div>
    {/if}
  {/each}

  {#if loaded && !samples.length && editing !== 'new'}
    <p class="empty">None yet. An email or two and a few messages you wrote are plenty.</p>
  {/if}

  {#if editing === 'new'}
    {@render form('Add')}
  {:else if samples.length < limits.max}
    <button class="add" bind:this={addEl} data-setting="writing-sample-add" onclick={() => openForm(null)}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
      Add a writing sample
    </button>
  {:else}
    <p class="full">That's {limits.max} writing samples, the most Yap keeps. Delete one to add another.</p>
  {/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  <p class="note">
    They stay on this PC. A rewrite carries the ones for the kind of app you're in, up to about {tokens(limits.budgetLocal)}
    tokens to a model on this PC and {tokens(limits.budgetCloud)} to a cloud one, and only then do they leave it. Never with Fix
    grammar only, Translate or Make it a prompt, and never with AI cleanup, which keeps your words as you said them.
  </p>
</div>

{#snippet form(verb)}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="form" bind:this={formEl} role="group" aria-label={verb === 'Add' ? 'New writing sample' : 'Edit writing sample'} onkeydown={onFormKey}>
    <label class="field">
      <span>Something you wrote</span>
      <Textarea
        bind:value={draftText}
        rows={5}
        maxlength={limits.maxChars}
        placeholder="Paste an email, a message or a paragraph you wrote yourself"
        ariaLabel="Writing sample"
      />
    </label>
    <div class="row2">
      <Select bind:value={draftKind} options={KINDS} ariaLabel="Kind of app it suits" />
      <span class="count">{draftText.length} / {limits.maxChars}</span>
    </div>
    <p class="hint">Yap shows the AI how you write, never what it says: names and facts in it are never copied.</p>
    <div class="actions">
      <Button variant="secondary" size="sm" onclick={closeForm}>Cancel</Button>
      <Button size="sm" disabled={!canSave} onclick={saveForm}>{verb === 'Add' ? 'Add sample' : 'Save'}</Button>
    </div>
  </div>
{/snippet}

<ConfirmDialog
  bind:open={confirmOpen}
  title="Delete this writing sample?"
  body={deleting ? `“${preview(deleting.text).slice(0, 80)}${preview(deleting.text).length > 80 ? '…' : ''}” (${kindName(deleting.kind)})` : ''}
  confirmLabel="Delete"
  onconfirm={doDelete}
/>

<style>
  .samples {
    display: flex;
    flex-direction: column;
  }
  .sample {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    column-gap: 16px;
    row-gap: 8px;
    padding: 10px 0;
    border-bottom: 1px solid var(--yap-rule);
  }
  .text {
    margin: 0;
    font-size: 13px;
    line-height: 1.45;
    color: var(--yap-fg-80);
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .rhs {
    display: flex;
    align-items: center;
    gap: 4px;
    justify-content: flex-end;
  }
  .tool {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    padding: 0;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-fg-62);
    cursor: pointer;
    transition:
      background var(--yap-dur) ease,
      color var(--yap-dur) ease;
  }
  .tool svg {
    width: 15px;
    height: 15px;
  }
  .tool:hover {
    background: var(--yap-s3);
    color: var(--yap-fg);
  }
  .tool.del:hover {
    color: var(--yap-danger);
  }
  .tool:focus-visible,
  .add:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
  }
  .add {
    align-self: flex-start;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    margin-top: 10px;
    height: 30px;
    padding: 0 12px 0 9px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
    box-shadow: var(--yap-shadow-sm);
    color: var(--yap-fg-80);
    font: inherit;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .add:hover {
    border-color: var(--yap-border-hover);
    color: var(--yap-fg);
  }
  .add svg {
    width: 14px;
    height: 14px;
  }
  .empty,
  .full,
  .note {
    margin: 10px 0 0;
    font-size: 12.5px;
    line-height: 1.45;
    color: var(--yap-muted);
  }
  .error {
    margin: 10px 0 0;
    font-size: 12.5px;
    color: var(--yap-danger);
  }
  .form {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin: 10px 0;
    padding: 14px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 5px;
  }
  .field > span {
    font-size: 12.5px;
    font-weight: 600;
    color: var(--yap-fg-80);
  }
  .row2 {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
  }
  .count {
    font-size: 12px;
    color: var(--yap-muted);
    font-variant-numeric: tabular-nums;
  }
  .hint {
    margin: 0;
    font-size: 12px;
    line-height: 1.45;
    color: var(--yap-muted);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  @media (max-width: 720px) {
    .sample {
      grid-template-columns: minmax(0, 1fr);
    }
    .rhs {
      justify-content: flex-start;
    }
  }
</style>
