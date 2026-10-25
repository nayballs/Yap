<script>
  // Settings → Commands → Transforms (dictation/transforms.rs; Wispr Flow's
  // Transforms): one-key rewrites of the selected text — or, nothing
  // selected, the last dictation — without saying anything. Yap's five
  // first (their instructions are Yap's: a name, a line, a shortcut, and
  // Translate's language), then the person's own (name + what it does, edited
  // in place, deleted behind Yap's confirm dialog). ↑/↓ reorder any of them:
  // the order is the tray's Transforms menu too. Each shortcut in Yap's
  // keycaps, checked against the seven fixed shortcuts and the other
  // transforms' (`validateTransformHotkey`), its live binding paused while
  // a key is picked. Last, "Run on every dictation" (`autoTransform`, off by
  // default; `transforms::for_dictation`): one transform after cleanup on
  // every dictation, or only in one kind of app. Auto-save does the saving
  // (Settings' copy of config).
  //
  //   cfg: Settings' config $state.
  import { tick } from 'svelte';
  import HotkeyInput from '../ui/HotkeyInput.svelte';
  import Select from '../ui/Select.svelte';
  import Input from '../ui/Input.svelte';
  import Textarea from '../ui/Textarea.svelte';
  import Button from '../ui/Button.svelte';
  import Row from '../ui/Row.svelte';
  import ConfirmDialog from '../ui/ConfirmDialog.svelte';
  import { onTransformCapturing, validateTransformHotkey } from './hotkeys.svelte.js';
  import { BUILTINS, EVERY_KINDS, LANGUAGES, MAX_TRANSFORMS, defaultTransforms, everyLine, newTransform, transformName } from './transforms.js';

  let { cfg } = $props();

  // A config from before transforms (or a stale backend): Yap's five.
  $effect.pre(() => {
    if (cfg && !Array.isArray(cfg.transforms)) cfg.transforms = defaultTransforms();
  });

  const list = $derived(Array.isArray(cfg?.transforms) ? cfg.transforms : []);
  const languageOptions = LANGUAGES.map((l) => ({ value: l, label: l }));
  const INSTRUCTION_MAX = 600;

  // The form: one transform being edited (its id), or 'new'; null: none.
  let editing = $state(null);
  let draftName = $state('');
  let draftText = $state('');
  let formEl = $state(null);
  let addEl = $state(null);
  // The transform about to be deleted (the confirm dialog's).
  let deleting = $state(null);
  let confirmOpen = $state(false);

  async function openForm(t) {
    editing = t ? t.id : 'new';
    draftName = t ? t.name : '';
    draftText = t ? t.instruction : '';
    await tick();
    formEl?.querySelector('input')?.focus();
  }

  function closeForm() {
    const was = editing;
    editing = null;
    tick().then(() => {
      const back = was === 'new' ? addEl : document.querySelector(`[data-transform="${was}"] .edit`);
      back?.focus?.({ preventScroll: true });
    });
  }

  const canSave = $derived(draftText.trim().length > 0);

  function save() {
    if (!canSave) return;
    if (editing === 'new') {
      const name = draftName.trim() || draftText.trim().split(/\s+/).slice(0, 4).join(' ');
      cfg.transforms = [...list, newTransform(name, draftText)];
    } else {
      const t = list.find((x) => x.id === editing);
      if (t) {
        t.name = draftName.trim().slice(0, 60);
        t.instruction = draftText.trim();
      }
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
      save();
    }
  }

  function move(i, by) {
    const j = i + by;
    if (j < 0 || j >= list.length) return;
    const next = [...list];
    [next[i], next[j]] = [next[j], next[i]];
    cfg.transforms = next;
  }

  function askDelete(t) {
    deleting = t;
    confirmOpen = true;
  }

  function doDelete() {
    if (deleting) cfg.transforms = list.filter((x) => x.id !== deleting.id);
    // The one on every dictation goes with it.
    if (deleting && cfg.autoTransform === deleting.id) cfg.autoTransform = '';
    confirmOpen = false;
    deleting = null;
    tick().then(() => addEl?.focus?.({ preventScroll: true }));
  }

  const line = (t) => (t.builtin ? BUILTINS[t.builtin]?.line || '' : t.instruction);

  // "Run on every dictation": one of the list (one deleted since: none).
  const everyT = $derived(list.find((t) => t.id === cfg.autoTransform) || null);
  const everyOptions = $derived([{ value: '', label: 'None' }, ...list.map((t) => ({ value: t.id, label: transformName(t) }))]);
  const everyKind = $derived(EVERY_KINDS.some((k) => k.value === cfg.autoTransformKind) ? cfg.autoTransformKind : '');
</script>

<div class="transforms" data-setting="transforms">
  {#each list as t, i (t.id)}
    {#if editing === t.id}
      {@render form('Save')}
    {:else}
      <div class="tf" data-transform={t.id}>
        <div class="lhs">
          <span class="name">{transformName(t)}</span>
          <span class="line" title={t.builtin ? undefined : t.instruction}>{line(t)}</span>
          {#if t.builtin === 'translate'}
            <div class="lang">
              <Select bind:value={t.language} options={languageOptions} ariaLabel="Translate to" />
            </div>
          {/if}
        </div>
        <div class="rhs">
          <HotkeyInput
            bind:value={t.hotkey}
            clearable
            validate={(s) => validateTransformHotkey(cfg, t.id, s)}
            oncapturingchange={(on) => onTransformCapturing(cfg, t.id, on)}
          />
          <div class="tools">
            <button class="tool" title="Move up" aria-label={`Move ${transformName(t)} up`} disabled={i === 0} onclick={() => move(i, -1)}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m6 15 6-6 6 6" /></svg>
            </button>
            <button
              class="tool"
              title="Move down"
              aria-label={`Move ${transformName(t)} down`}
              disabled={i === list.length - 1}
              onclick={() => move(i, 1)}
            >
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m6 9 6 6 6-6" /></svg>
            </button>
            {#if !t.builtin}
              <button class="tool edit" title="Edit" aria-label={`Edit ${transformName(t)}`} onclick={() => openForm(t)}>
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 20h4L19 9l-4-4L4 16v4z" /><path d="m13.5 6.5 4 4" /></svg>
              </button>
              <button class="tool del" title="Delete" aria-label={`Delete ${transformName(t)}`} onclick={() => askDelete(t)}>
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 7h14M10 7V5h4v2M7 7l1 12h8l1-12" /></svg>
              </button>
            {:else}
              <!-- Yap's own: no Edit or Delete, their places kept so every
                   shortcut lines up. -->
              <span class="tool ph" aria-hidden="true"></span>
              <span class="tool ph" aria-hidden="true"></span>
            {/if}
          </div>
        </div>
      </div>
    {/if}
  {/each}

  {#if editing === 'new'}
    {@render form('Add')}
  {:else if list.length < MAX_TRANSFORMS}
    <button class="add" bind:this={addEl} data-setting="transform-add" onclick={() => openForm(null)}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
      Add a transform
    </button>
  {:else}
    <p class="full" data-setting="transform-add">That's {MAX_TRANSFORMS} transforms, the most Yap keeps. Delete one to add another.</p>
  {/if}
</div>

<Row
  label="Run on every dictation"
  desc={everyLine(everyT, everyKind)}
  hint="One more AI call for each dictation, on the Commands model. “Use what I said” puts back your own words."
  setting="transform-every"
  stack
>
  <div class="every">
    <Select bind:value={() => (everyT ? everyT.id : ''), (v) => (cfg.autoTransform = v)} options={everyOptions} ariaLabel="Transform on every dictation" />
    <Select bind:value={() => everyKind, (v) => (cfg.autoTransformKind = v)} options={EVERY_KINDS} ariaLabel="Where it runs" disabled={!everyT} />
  </div>
</Row>

{#snippet form(verb)}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="form" bind:this={formEl} role="group" aria-label={verb === 'Add' ? 'New transform' : 'Edit transform'} onkeydown={onFormKey}>
    <label class="field">
      <span>Name</span>
      <Input bind:value={draftName} placeholder="Friendlier" maxlength={60} ariaLabel="Transform name" />
    </label>
    <label class="field">
      <span>What it does</span>
      <Textarea
        bind:value={draftText}
        rows={3}
        maxlength={INSTRUCTION_MAX}
        placeholder="Make it warmer and friendlier, as if to a colleague"
        ariaLabel="What the transform does"
      />
    </label>
    <p class="hint">Say what to change. Yap tells the AI to change only the text, never to answer it.</p>
    <div class="actions">
      <Button variant="secondary" size="sm" onclick={closeForm}>Cancel</Button>
      <Button size="sm" disabled={!canSave} onclick={save}>{verb === 'Add' ? 'Add transform' : 'Save'}</Button>
    </div>
  </div>
{/snippet}

<ConfirmDialog
  bind:open={confirmOpen}
  title={deleting ? `Delete “${transformName(deleting)}”?` : ''}
  body="Its shortcut goes with it."
  confirmLabel="Delete"
  onconfirm={doDelete}
/>

<style>
  .transforms {
    display: flex;
    flex-direction: column;
  }
  .tf {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    column-gap: 20px;
    row-gap: 8px;
    padding: 12px 0;
    border-bottom: 1px solid var(--yap-rule);
  }
  .lhs {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .name {
    font-size: 13.5px;
    font-weight: 560;
    color: var(--yap-fg);
  }
  .line {
    font-size: 12.5px;
    line-height: 1.45;
    color: var(--yap-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .lang {
    margin-top: 6px;
    width: var(--yap-select-w);
  }
  .rhs {
    display: flex;
    align-items: center;
    gap: 10px;
    justify-content: flex-end;
  }
  .tools {
    display: flex;
    align-items: center;
    gap: 0;
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
  .tool:hover:not(:disabled):not(.ph) {
    background: var(--yap-s3);
    color: var(--yap-fg);
  }
  .tool.del:hover:not(:disabled) {
    color: var(--yap-danger);
  }
  .tool:disabled {
    opacity: 0.35;
    cursor: default;
  }
  .tool.ph {
    cursor: default;
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
  .full {
    margin: 10px 0 0;
    font-size: 12.5px;
    color: var(--yap-muted);
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
  .every {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  @media (max-width: 720px) {
    .tf {
      grid-template-columns: minmax(0, 1fr);
    }
    .rhs {
      justify-content: flex-start;
    }
  }
</style>
