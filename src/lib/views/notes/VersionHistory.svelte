<script>
  // A note's Version history (notes/versions.rs): its earlier versions,
  // newest first — My thoughts' and the summary's, each with how it came
  // (typed, dictated, Yap's summary, an Action, a transform, Chat,
  // restored) and when — beside a preview with the changes marked
  // (core/wordDiff.js, word by word, capitals and punctuation too): what
  // that version changed, or how it differs from the note now. Restore puts
  // it back as a version of its own, so Undo (the toast) or this list takes
  // it back again. ↑ ↓ move through the list; Esc closes.
  import { invoke } from '@tauri-apps/api/core';
  import { tick } from 'svelte';
  import { wordDiff } from '../../core/wordDiff.js';
  import { menuOrDialogOpen } from '../../ui/contextMenu.svelte.js';

  /** `notes`: the view's state (notesState.svelte.js). */
  let { notes } = $props();

  let versions = $state([]);
  let loading = $state(false);
  let error = $state(null);
  let at = $state(0);
  // 'before': what this version changed; 'now': how it differs from the note now.
  let compare = $state('before');
  let restoring = $state(false);
  let rowEls = $state([]);

  const open = $derived(notes.historyOpen && !!notes.selected);
  const v = $derived(versions[at] ?? null);
  // The version before it, of the same text.
  const prev = $derived(v ? versions.slice(at + 1).find((x) => x.field === v.field) ?? null : null);
  const nowText = $derived(v ? (v.field === 'summary' ? notes.selected?.enhancedContent : notes.selected?.content) ?? '' : '');
  const diff = $derived.by(() => {
    if (!v) return null;
    return compare === 'now' ? wordDiff(nowText, v.text, { exact: true }) : wordDiff(prev?.text ?? '', v.text, { exact: true });
  });

  let loadedFor = null;
  $effect(() => {
    const id = open ? notes.selected.id : null;
    if (id === loadedFor) return;
    loadedFor = id;
    if (id != null) load(id);
  });

  async function load(id) {
    loading = true;
    error = null;
    try {
      versions = (await invoke('note_versions', { id })) || [];
    } catch (e) {
      versions = [];
      error = String(e);
    } finally {
      loading = false;
    }
    if (loadedFor !== id) return;
    at = 0;
    compare = 'before';
    await tick();
    rowEls[0]?.focus();
  }

  function close() {
    loadedFor = null;
    versions = [];
    notes.closeHistory();
  }

  async function restore() {
    if (!v || v.current || restoring) return;
    restoring = true;
    try {
      const r = await notes.restoreVersion(v.n);
      if (r) close();
    } finally {
      restoring = false;
    }
  }

  function pick(i) {
    at = Math.max(0, Math.min(versions.length - 1, i));
    rowEls[at]?.focus();
  }

  function onListKeydown(e) {
    if (e.key === 'ArrowDown') pick(at + 1);
    else if (e.key === 'ArrowUp') pick(at - 1);
    else if (e.key === 'Home') pick(0);
    else if (e.key === 'End') pick(versions.length - 1);
    else return;
    e.preventDefault();
  }

  function onWindowKeydown(e) {
    if (!open || e.key !== 'Escape' || menuOrDialogOpen()) return;
    e.preventDefault();
    e.stopPropagation();
    close();
  }

  /** How a version came, in words. */
  function kindLabel(x) {
    switch (x.kind) {
      case 'typed':
        return 'Typed';
      case 'dictated':
        return 'Dictated';
      case 'summary':
        return 'Summary by Yap';
      case 'action':
        return x.label ? `Action: ${x.label}` : 'Action';
      case 'transform':
        return x.label || 'Transform';
      case 'chat':
        return 'Changed by Chat';
      case 'restored':
        if (x.label) return `Back to ${x.label}`;
        return 'Restored';
      case 'earlier':
        return 'Earlier';
      default:
        return 'Changed';
    }
  }
  /** What a restore brought back: "from 14:02". */
  function restoredFrom(x) {
    if (x.kind !== 'restored' || x.from == null) return '';
    const from = versions.find((y) => y.n === x.from);
    return from ? `from ${when(from.ms)}` : '';
  }
  const fieldLabel = (x) => (x.field === 'summary' ? 'Summary' : 'My thoughts');

  /** "14:02", "Yesterday, 14:02", "28 Oct, 14:02". */
  function when(ms) {
    const d = new Date(ms);
    const time = d.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' });
    const today = new Date();
    if (d.toDateString() === today.toDateString()) return time;
    const yesterday = new Date(today);
    yesterday.setDate(today.getDate() - 1);
    if (d.toDateString() === yesterday.toDateString()) return `Yesterday, ${time}`;
    return `${d.toLocaleDateString(undefined, { day: 'numeric', month: 'short' })}, ${time}`;
  }
  /** Saves folded into one version: "14:02–14:09". */
  function span(x) {
    const a = when(x.sinceMs);
    const b = when(x.ms);
    if (a === b) return a;
    const sameDay = new Date(x.sinceMs).toDateString() === new Date(x.ms).toDateString();
    return sameDay ? `${a}–${new Date(x.ms).toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' })}` : `${a} – ${b}`;
  }
</script>

<svelte:window onkeydown={onWindowKeydown} />

{#if open}
  <div class="backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && close()}>
    <div class="dialog" role="dialog" aria-modal="true" aria-label="Version history">
      <button class="x" aria-label="Close" onclick={close}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
      </button>

      <div class="pane list">
        <div class="listhead">Version history</div>
        {#if loading && !versions.length}
          <p class="quiet">Loading…</p>
        {:else if error}
          <p class="quiet err">{error}</p>
        {:else if !versions.length}
          <div class="empty">
            <p><strong>No earlier versions yet</strong></p>
            <p>Yap keeps a version as you type or dictate, and whenever the AI rewrites this note: the last 50.</p>
          </div>
        {:else}
          <div class="items" role="listbox" aria-label="Versions" tabindex="-1" onkeydown={onListKeydown}>
            {#each versions as x, i (x.n)}
              <button
                bind:this={rowEls[i]}
                class="item"
                class:active={i === at}
                role="option"
                aria-selected={i === at}
                tabindex={i === at ? 0 : -1}
                data-version={x.n}
                onclick={() => pick(i)}
              >
                <span class="irow">
                  <span class="ikind">{kindLabel(x)}</span>
                  {#if x.current}<span class="badge">Current</span>{/if}
                </span>
                <span class="imeta">
                  <span class="ifield" class:summary={x.field === 'summary'}>{fieldLabel(x)}</span>
                  · {span(x)}{#if restoredFrom(x)} · {restoredFrom(x)}{/if}
                </span>
              </button>
            {/each}
          </div>
        {/if}
      </div>

      <div class="pane preview">
        {#if v}
          <div class="phead">
            <div class="ptitle">
              <span class="pkind">{kindLabel(v)}</span>
              <span class="pmeta">{fieldLabel(v)} · {span(v)}</span>
            </div>
            <div class="seg" role="radiogroup" aria-label="Changes marked">
              <button role="radio" aria-checked={compare === 'before'} class:on={compare === 'before'} onclick={() => (compare = 'before')}>
                What it changed
              </button>
              <button role="radio" aria-checked={compare === 'now'} class:on={compare === 'now'} onclick={() => (compare = 'now')}>
                Compared with now
              </button>
            </div>
          </div>
          <p class="legend" aria-live="polite">
            {#if compare === 'now'}
              {#if v.current}This is the note as it is now.{:else if !diff.changes}The same words as the note now.{:else}Restoring takes out what's struck and puts back what's marked.{/if}
            {:else if !prev}
              {v.kind === 'earlier' ? 'What the note said before its history began.' : 'The first version kept.'}
            {:else if !diff.changes}
              Nothing changed but spacing.
            {:else}
              {diff.changes === 1 ? '1 change' : `${diff.changes} changes`} from the version before.
            {/if}
          </p>
          <!-- Focusable, so the keyboard scrolls a long version. -->
          <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
          <div class="pbody" tabindex="0" role="region" aria-label="The version's text, its changes marked">
            {#if !v.text.trim() && !(diff?.parts || []).some((p) => p.type === 'del')}
              <p class="quiet">(empty)</p>
            {:else}
              <p class="difftext">{#each diff.parts as p, k}{k ? p.sep : ''}{#if p.type === 'del'}<del><span class="sr">{'taken out: '}</span>{p.text}</del>{:else if p.type === 'add'}<ins><span class="sr">{'put in: '}</span>{p.text}</ins>{:else}{p.text}{/if}{/each}</p>
            {/if}
          </div>
          <div class="pfoot">
            <span class="hint">{v.current ? "It's the note as it is now." : 'The text it replaces stays in the history.'}</span>
            <button class="restore" disabled={v.current || restoring} onclick={restore}>
              {restoring ? 'Restoring…' : 'Restore this version'}
            </button>
          </div>
        {/if}
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
    background: var(--yap-scrim);
  }
  .dialog {
    position: relative;
    display: flex;
    width: min(820px, calc(100vw - 60px));
    height: min(540px, calc(100vh - 80px));
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
    flex: 0 0 250px;
    display: flex;
    flex-direction: column;
    min-height: 0;
    border-right: 1px solid var(--yap-border-subtle);
    background: var(--yap-s0, var(--yap-s1));
  }
  .listhead {
    padding: 14px 14px 8px;
    font-size: 12px;
    font-weight: 600;
    color: var(--yap-fg);
  }
  .items {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    padding: 0 8px 12px;
  }
  .item {
    display: flex;
    flex-direction: column;
    gap: 2px;
    width: 100%;
    padding: 7px 10px;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-fg);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .item:hover {
    background: var(--yap-s2);
  }
  .item.active {
    background: var(--yap-primary-wash);
  }
  .item:focus-visible {
    outline: 1.5px solid var(--yap-primary-line);
    outline-offset: -1px;
  }
  .irow {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .ikind {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12.5px;
    font-weight: 600;
  }
  .badge {
    flex: 0 0 auto;
    padding: 1px 6px;
    border-radius: var(--yap-r-sm);
    background: var(--yap-s3);
    color: var(--yap-muted);
    font-size: 10px;
    font-weight: 600;
  }
  .imeta {
    font-size: 10.5px;
    color: var(--yap-muted-55);
  }
  .ifield.summary {
    color: var(--yap-primary);
  }
  .quiet {
    margin: 4px 14px;
    font-size: 12px;
    color: var(--yap-muted);
  }
  .quiet.err {
    color: var(--yap-danger);
  }
  .empty {
    padding: 6px 14px;
    font-size: 12px;
    line-height: 1.55;
    color: var(--yap-muted);
  }
  .empty p {
    margin: 0 0 6px;
  }
  .empty strong {
    color: var(--yap-fg);
  }

  .pane.preview {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    padding: 14px 18px 14px;
  }
  .phead {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 10px;
    padding-right: 30px;
  }
  .ptitle {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .pkind {
    font-size: 14px;
    font-weight: 700;
    color: var(--yap-fg);
  }
  .pmeta {
    font-size: 11px;
    color: var(--yap-muted);
  }
  .seg {
    display: inline-flex;
    padding: 2px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
  }
  .seg button {
    height: 24px;
    padding: 0 10px;
    border: none;
    border-radius: var(--yap-r-sm);
    background: none;
    color: var(--yap-muted);
    font: inherit;
    font-size: 11px;
    cursor: pointer;
  }
  .seg button.on {
    background: var(--yap-s1);
    color: var(--yap-fg);
    font-weight: 600;
  }
  .seg button:focus-visible {
    outline: 1.5px solid var(--yap-primary-line);
  }
  .legend {
    margin: 10px 0 8px;
    font-size: 11.5px;
    color: var(--yap-muted);
  }
  .pbody {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    padding: 10px 12px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s0, var(--yap-s1));
  }
  .pbody:focus-visible {
    outline: 1.5px solid var(--yap-primary-line);
  }
  .difftext {
    margin: 0;
    font-family: var(--yap-font-reading);
    font-size: calc(13px * var(--yap-reading-scale));
    line-height: 1.7;
    color: var(--yap-fg);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .difftext del {
    color: var(--yap-danger);
    text-decoration: line-through;
    text-decoration-thickness: 1.5px;
    background: color-mix(in srgb, var(--yap-danger) 9%, transparent);
    border-radius: 3px;
    padding: 0 2px;
  }
  .difftext ins {
    color: var(--yap-fg);
    text-decoration: underline;
    text-decoration-color: var(--yap-primary);
    text-decoration-thickness: 2px;
    text-underline-offset: 3px;
    background: var(--yap-primary-wash);
    border-radius: 3px;
    padding: 0 2px;
  }
  .sr {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
  .pfoot {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-top: 12px;
  }
  .hint {
    flex: 1 1 auto;
    font-size: 11px;
    color: var(--yap-muted-55);
  }
  .restore {
    height: 30px;
    padding: 0 14px;
    border: none;
    border-radius: var(--yap-r);
    background: var(--yap-ink, var(--yap-primary));
    color: var(--yap-ink-fg, var(--yap-primary-fg));
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .restore:hover:not(:disabled) {
    background: var(--yap-ink-hover, var(--yap-primary-hover));
  }
  .restore:disabled {
    opacity: 0.45;
    cursor: default;
  }
</style>
