<script>
  // **A Code chat's file beside the chat** (15 Nov 2026, after the Claude
  // app's Code tab: a path in an answer opens the file): read only, its lines
  // numbered and in colour (core/codeColours.js: highlight.js in its own
  // chunk, its tokens as text, never markup), scrolled to the line a link
  // named and that line marked. Opened by a file link in an answer or a row
  // (ui/fileLinks.js, StepRows.svelte), or the summary's Outputs.
  //
  // Rust reads it (agents/files.rs `code_file_read`): only a file in the
  // chat's own workspace, at most 1 MB and 20,000 lines (the rest said), a
  // file that isn't text said so. Read again each time it's opened (a link
  // after Claude changed it) and with ↻.
  //
  // - **Copy path**: its full path. **Open in editor**: VS Code or Cursor at
  //   the line, else Notepad++ or Notepad (`code_file_open_editor`; never
  //   the file's own default app, which could run it).
  import { invoke } from '@tauri-apps/api/core';
  import { tick, untrack } from 'svelte';
  import { colourLines, languageOf, loadColours } from '../../core/codeColours.js';
  import { copyText } from '../../core/itemMenus.js';
  import { toast } from '../../ui/toast.svelte.js';

  /** `doc`: the panel's state (`doc.file`: `{rel, line, seq}`); `conversationId`: the Code chat. */
  let { doc, narrow = false, conversationId = null } = $props();

  const file = $derived(doc.file);
  let view = $state(null);
  let error = $state('');
  let loading = $state(false);
  let colours = $state.raw(null);
  let bodyEl = $state(null);
  let reading = 0;

  async function load() {
    const conv = conversationId;
    const rel = file?.rel;
    if (conv == null || !rel) return;
    const mine = ++reading;
    loading = true;
    try {
      const v = await invoke('code_file_read', { conversation: conv, path: rel });
      if (mine !== reading) return;
      view = v;
      error = '';
      if (!v.binary && languageOf(v.rel) && !colours) {
        loadColours()
          .then((h) => (colours = h))
          .catch(() => {});
      }
    } catch (e) {
      if (mine !== reading) return;
      view = null;
      error = String(e?.message ?? e);
    } finally {
      if (mine === reading) loading = false;
    }
    await tick();
    toLine();
  }

  // Opened (again: `seq`), another file, another chat: read it.
  $effect(() => {
    void file?.rel;
    void file?.seq;
    void conversationId;
    untrack(load);
  });

  /** The line asked for, in the middle of the view. */
  function toLine() {
    const n = file?.line;
    const el = n ? bodyEl?.querySelector(`[data-no="${n}"]`) : null;
    if (el) el.scrollIntoView({ block: 'center' });
    else if (bodyEl) bodyEl.scrollTop = 0;
  }

  // The file's lines (its last newline isn't a line of its own).
  const lines = $derived(view && !view.binary ? view.text.replace(/\n$/, '').replace(/\r/g, '').split('\n') : []);
  const tokens = $derived(colours && lines.length ? colourLines(colours, lines.join('\n'), languageOf(view?.rel)) : null);
  const digits = $derived(String(lines.length || 1).length);
  const name = $derived((file?.rel || '').split('/').pop());
  const folder = $derived((file?.rel || '').split('/').slice(0, -1).join('/'));
  const size = (n) => (n >= 1e6 ? `${(n / 1e6).toFixed(1)} MB` : n >= 1e3 ? `${Math.round(n / 1e3)} KB` : `${n} bytes`);
  const facts = $derived(
    view
      ? view.binary
        ? size(view.size)
        : `${lines.length.toLocaleString('en-GB')} ${lines.length === 1 ? 'line' : 'lines'} · ${size(view.size)}${view.truncated ? ' · only its start is shown' : ''}`
      : '',
  );

  async function openInEditor() {
    try {
      await invoke('code_file_open_editor', { conversation: conversationId, path: file.rel, line: file.line ?? null });
    } catch (e) {
      toast({ title: "Couldn't open it", description: String(e?.message ?? e), variant: 'destructive' });
    }
  }
</script>

<section class="filetab" aria-label="File" data-testid="chat-file" data-rel={file?.rel ?? ''} data-line={file?.line ?? ''}>
  <header class="filehead">
    <svg class="fileicon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z" /><path d="M14 3v5h5" /><path d="m10 13-2 2 2 2M14 13l2 2-2 2" /></svg>
    <h2 class="filetitle" title={view?.abs || file?.rel}>
      <span class="fname" data-testid="chat-file-name">{name}</span>{#if folder}<span class="fdir">{folder}</span>{/if}
    </h2>
    {#if file?.line}<span class="lineno" data-testid="chat-file-line">Line {file.line}</span>{/if}
    <div class="fileactions">
      <button type="button" class="fbtn" onclick={load} aria-label="Read it again" title="Read it again" disabled={loading}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M20 11a8 8 0 1 0-2.3 5.7" /><path d="M20 4v7h-7" /></svg>
      </button>
      <button type="button" class="fbtn" onclick={() => copyText(view?.abs || file?.rel || '', 'Path copied')} aria-label="Copy path" title="Copy path" data-testid="chat-file-copy-path">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="8" y="8" width="12" height="12" rx="2" /><path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2" /></svg>
      </button>
      <button type="button" class="fbtn wide" onclick={openInEditor} title="Open in your code editor" data-testid="chat-file-open-editor" disabled={!view}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M14 4h6v6M20 4l-9 9" /><path d="M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5" /></svg>
        <span>Open in editor</span>
      </button>
      {#if !narrow}
        <button
          type="button"
          class="fbtn"
          onclick={() => (doc.expanded = !doc.expanded)}
          aria-label={doc.expanded ? 'Back beside the chat' : 'Expand to full width'}
          title={doc.expanded ? 'Back beside the chat' : 'Expand to full width'}
        >
          {#if doc.expanded}
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 14h6v6M20 10h-6V4M14 10l7-7M3 21l7-7" /></svg>
          {:else}
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M15 3h6v6M9 21H3v-6M21 3l-7 7M3 21l7-7" /></svg>
          {/if}
        </button>
      {/if}
      <button type="button" class="fbtn" onclick={() => doc.close()} aria-label="Close the panel" title="Close">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
      </button>
    </div>
  </header>
  {#if error}
    <p class="fnote" role="alert" data-testid="chat-file-error">{error}</p>
  {:else if view?.binary}
    <p class="fnote" data-testid="chat-file-binary">This file isn't text, so Yap doesn't show it here. Open in editor shows it in your editor.</p>
  {:else if view}
    <!-- (A box that scrolls takes the keyboard's focus, so its arrows can scroll it.) -->
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <div class="fbody" bind:this={bodyEl} tabindex="0" role="region" aria-label={`${file?.rel} (read only)`} style:--digits={digits} data-coloured={tokens ? 'true' : 'false'} data-testid="chat-file-body">
      {#each lines as text, i (i)}
        <div class="fl" class:mark={i + 1 === file?.line} data-no={i + 1}>
          <span class="fno" aria-hidden="true">{i + 1}</span><span class="ft"
            >{#if tokens?.[i]}{#each tokens[i] as t, k (k)}<span class={t.kind ? `syn-${t.kind}` : undefined}>{t.text}</span>{/each}{:else}{text || ' '}{/if}</span
          >
        </div>
      {/each}
    </div>
  {:else if loading}
    <p class="fnote">Reading…</p>
  {/if}
  {#if facts}<p class="ffacts" data-testid="chat-file-facts">{facts}</p>{/if}
</section>

<style>
  .filetab {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    color: var(--yap-paper-ink);
  }
  .filehead {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 12px 8px 18px;
    border-bottom: 1px solid var(--yap-paper-rule);
  }
  .fileicon {
    flex: 0 0 auto;
    width: 15px;
    height: 15px;
    color: var(--yap-code);
  }
  .filetitle {
    display: flex;
    align-items: baseline;
    gap: 8px;
    flex: 0 1 auto;
    min-width: 0;
    margin: 0;
    overflow: hidden;
    white-space: nowrap;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--yap-paper-text-2);
  }
  .fname {
    flex: 0 0 auto;
    font-family: ui-monospace, Consolas, monospace;
  }
  .fdir {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    direction: rtl;
    font-weight: 400;
    font-size: 11.5px;
    color: var(--yap-paper-muted);
  }
  .lineno {
    flex: 0 0 auto;
    padding: 1px 8px;
    border-radius: 999px;
    background: var(--yap-paper-s1);
    font-size: 11px;
    font-weight: 600;
    color: var(--yap-paper-muted);
  }
  .fileactions {
    display: flex;
    align-items: center;
    gap: 2px;
    margin-left: auto;
  }
  .fbtn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    height: 28px;
    min-width: 28px;
    padding: 0;
    border: 1px solid transparent;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-paper-muted);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }
  .fbtn.wide {
    padding: 0 8px;
    border-color: var(--yap-paper-rule);
    color: var(--yap-paper-text-2);
  }
  .fbtn:hover:not(:disabled) {
    background: var(--yap-paper-s2);
    color: var(--yap-paper-ink);
  }
  .fbtn:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .fbtn:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  .fbtn svg {
    width: 15px;
    height: 15px;
    flex: 0 0 auto;
  }
  .fbody {
    flex: 1 1 auto;
    min-height: 0;
    overflow: auto;
    padding: 6px 0 10px;
    font-family: ui-monospace, Consolas, monospace;
    font-size: calc(11.5px * var(--yap-reading-step, 1));
    line-height: 1.55;
    background: var(--yap-paper);
  }
  .fbody:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: -2px;
  }
  .fl {
    display: flex;
    min-width: max-content;
    padding-right: 14px;
  }
  .fl.mark {
    background: var(--yap-primary-tint);
    box-shadow: inset 2px 0 0 var(--yap-primary);
  }
  .fno {
    flex: 0 0 auto;
    width: calc(var(--digits) * 1ch + 22px);
    padding-right: 12px;
    text-align: right;
    color: var(--yap-paper-faint);
    user-select: none;
  }
  .fl.mark .fno {
    color: var(--yap-primary);
    font-weight: 600;
  }
  .ft {
    white-space: pre;
    tab-size: 4;
  }
  .fnote {
    margin: 18px;
    color: var(--yap-paper-muted);
    font-size: 13px;
  }
  .ffacts {
    margin: 0;
    padding: 6px 18px;
    border-top: 1px solid var(--yap-paper-rule);
    color: var(--yap-paper-muted);
    font-size: 11.5px;
  }
</style>
