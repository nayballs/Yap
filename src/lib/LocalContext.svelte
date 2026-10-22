<script>
  // The on-device model's context: how much text it reads at once (chat
  // history, notes, a meeting), shared by Chat, dictation cleanup and the
  // meeting notes — one sidecar serves them all. In the Chat model picker's
  // On this PC pane (`compact`) and in Settings → Language Models → Local.
  //
  // The choices go up to what the model was trained for, each with what it
  // takes (local_llm.rs `context_view`: the GGUF's shape, calibrated
  // against llamafile's log), and a warning on any that won't fit on the
  // graphics card. Changing it saves `localContext` (local_llm_set_context,
  // as Settings saves) and restarts the model once no dictation is going
  // on; Settings' copy adopts it from `yap-local-context-changed`.
  import { invoke } from '@tauri-apps/api/core';
  import { fmtTokens, optionLabel, memoryLine, measured, overLine, pendingLine } from './localContext.js';

  let { status = null, compact = false, disabled = false, onchange = null } = $props();

  const ctx = $derived(status?.context ?? null);
  let saving = $state(false);
  let error = $state('');
  const id = `lctx-${Math.random().toString(36).slice(2, 8)}`;

  // The picker's pane is narrow: there the select's own "≈ 0.9 GB" says it,
  // and the line only shows what the running model's log measured.
  const mem = $derived(compact && !measured(ctx) ? '' : memoryLine(ctx));
  const over = $derived(overLine(ctx));
  const pending = $derived(pendingLine(ctx));
  const note = $derived(
    [
      'Longer context: longer chats and meeting notes fit, more graphics memory, a little slower.',
      ctx?.max ? `This model goes up to ${fmtTokens(ctx.max)}.` : '',
      compact ? '' : 'Shared by Chat, dictation cleanup and meeting notes.',
    ]
      .filter(Boolean)
      .join(' ')
  );

  async function pick(e) {
    const tokens = Number(e.currentTarget.value);
    if (!ctx || tokens === ctx.tokens || saving) return;
    saving = true;
    error = '';
    try {
      const view = await invoke('local_llm_set_context', { tokens });
      onchange?.(view);
    } catch (err) {
      error = String(err);
      // Back to what's saved.
      e.currentTarget.value = String(ctx.tokens);
    } finally {
      saving = false;
    }
  }
</script>

{#if ctx}
  <div class="lctx" class:compact data-testid="local-context">
    <div class="row">
      <label class="lbl" for={id}>Context</label>
      <span class="sel" class:off={disabled || saving}>
        <select
          {id}
          value={ctx.tokens}
          disabled={disabled || saving}
          aria-describedby="{id}-note"
          onchange={pick}
        >
          {#each ctx.options as o (o.tokens)}
            <option value={o.tokens}>{optionLabel(o)}</option>
          {/each}
        </select>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M6 9l6 6 6-6" /></svg>
      </span>
      {#if mem}<span class="mem" data-testid="local-context-memory">{mem}</span>{/if}
    </div>
    <p class="note" id="{id}-note">{note}</p>
    {#if over}<p class="warn" role="status" data-testid="local-context-over">{over}</p>{/if}
    {#if pending}<p class="pending" role="status" data-testid="local-context-pending"><span class="spin" aria-hidden="true"></span>{pending}</p>{/if}
    {#if error}<p class="warn" role="alert">{error}</p>{/if}
  </div>
{/if}

<style>
  .lctx {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 10px 12px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: 12px;
    background: var(--yap-s1);
    font-size: 12.5px;
  }
  .lctx.compact {
    margin: 0 10px 6px;
    padding: 8px 10px;
  }
  .row {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px 10px;
  }
  .lbl {
    color: var(--yap-fg);
    font-weight: 600;
  }
  .sel {
    position: relative;
    display: inline-flex;
    align-items: center;
  }
  select {
    appearance: none;
    height: 28px;
    padding: 0 28px 0 10px;
    border: 1px solid var(--yap-border);
    border-radius: 8px;
    background: var(--yap-s2, var(--yap-bg));
    color: var(--yap-fg);
    font: inherit;
    font-size: 12.5px;
    cursor: pointer;
  }
  select:hover:not(:disabled) {
    border-color: var(--yap-border-hover);
  }
  select:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  select:disabled {
    cursor: default;
    opacity: 0.6;
  }
  .sel svg {
    position: absolute;
    right: 8px;
    width: 13px;
    height: 13px;
    color: var(--yap-muted);
    pointer-events: none;
  }
  .mem {
    color: var(--yap-muted);
  }
  .note {
    margin: 0;
    color: var(--yap-muted);
    font-size: 12px;
    line-height: 1.45;
  }
  .warn {
    margin: 2px 0 0;
    color: var(--yap-warning);
    font-size: 12px;
    line-height: 1.45;
  }
  .pending {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 2px 0 0;
    color: var(--yap-muted);
    font-size: 12px;
  }
  .spin {
    width: 11px;
    height: 11px;
    border: 2px solid color-mix(in srgb, var(--yap-muted) 35%, transparent);
    border-top-color: var(--yap-muted);
    border-radius: 50%;
    flex: 0 0 auto;
  }
  @media (prefers-reduced-motion: no-preference) {
    .spin {
      animation: lctx-spin 0.8s linear infinite;
    }
  }
  @keyframes lctx-spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
