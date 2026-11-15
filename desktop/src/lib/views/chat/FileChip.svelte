<script>
  // **A file attached to a question** (chat/attachments.rs, 15 Nov 2026), as the Claude app's file chips look: a
  // small type badge ("PDF", "MD", "TXT"), its name and its size. In Chat's box it has ✕; on a sent question it opens
  // the file (Markdown in Yap's own window, anything else in its app; what could run a program shows its text).
  // Past "Keep chat files": greyed, "removed".
  import { invoke } from '@tauri-apps/api/core';
  import { getContext } from 'svelte';
  import { SHEETS } from './sheetView.js';
  import { toast } from '../../ui/toast.svelte.js';
  import { badgeOf, fileSize, fileTitle } from './chatFiles.js';

  /** `file`: an Attachment ({ id, name, kind, bytes, tokens, pages?, gone? }); `onremove`: shown in the box (✕). */
  let { file, onremove = null } = $props();

  const badge = $derived(badgeOf(file));
  let opening = $state(false);
  // A spreadsheet opens beside the chat (SheetTab), where Chat has the panel.
  const sheets = getContext(SHEETS) ?? null;

  async function open() {
    if (file.gone || opening) return;
    if (file.kind === 'sheet' && sheets) {
      sheets.open(file);
      return;
    }
    opening = true;
    try {
      await invoke('chat_attachment_open', { id: file.id });
    } catch (e) {
      toast({ title: `Couldn't open ${file.name}`, description: String(e), variant: 'destructive' });
    } finally {
      opening = false;
    }
  }
</script>

<span class="filechip" class:gone={file.gone} class:inbox={!!onremove} data-testid="chat-file-chip" data-kind={file.kind}>
  <button type="button" class="fbody" title={fileTitle(file)} disabled={file.gone} onclick={open} aria-label={`${file.name}, ${fileSize(file.bytes)}${file.gone ? ', removed' : ''}. Open it`}>
    <span class="fbadge" aria-hidden="true">{badge}</span>
    <span class="fname">{file.name}</span>
    <span class="fsize">{file.gone ? 'removed' : fileSize(file.bytes)}</span>
  </button>
  {#if onremove}
    <button type="button" class="fx" aria-label={`Remove ${file.name}`} title="Remove" onclick={() => onremove(file.id)}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" aria-hidden="true"><path d="M7 7l10 10M17 7 7 17" /></svg>
    </button>
  {/if}
</span>

<style>
  .filechip {
    position: relative;
    display: inline-flex;
    align-items: center;
    max-width: 100%;
    min-width: 0;
    border: 1px solid var(--yap-border);
    border-radius: 9px;
    background: var(--yap-s1);
    color: var(--yap-fg);
  }
  .filechip:hover:not(.gone) {
    border-color: var(--yap-border-hover, var(--yap-border));
  }
  .fbody {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    min-width: 0;
    padding: 5px 9px 5px 7px;
    border: none;
    border-radius: inherit;
    background: none;
    color: inherit;
    font: inherit;
    font-size: 12.5px;
    cursor: pointer;
  }
  .fbody:disabled {
    cursor: default;
  }
  .fbody:focus-visible,
  .fx:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  /* The type, a small outlined pill (the Claude app's "SVG", "MD"). */
  .fbadge {
    flex: 0 0 auto;
    padding: 0 5px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r-full);
    color: var(--yap-muted);
    font-size: 9px;
    font-weight: 600;
    line-height: 15px;
    letter-spacing: 0.02em;
  }
  .fname {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 600;
  }
  .fsize {
    flex: 0 0 auto;
    color: var(--yap-muted);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }
  .gone .fname {
    color: var(--yap-muted);
    font-weight: 500;
  }
  .gone .fbadge {
    opacity: 0.6;
  }
  /* In the box: ✕ out over the corner on hover, as a screenshot tile's. */
  .inbox .fbody {
    padding-right: 10px;
  }
  .fx {
    position: absolute;
    top: -7px;
    right: -7px;
    width: 18px;
    height: 18px;
    display: grid;
    place-items: center;
    padding: 0;
    border: none;
    border-radius: 50%;
    background: var(--yap-s2);
    box-shadow: inset 0 0 0 var(--yap-hairline) var(--yap-shot-ring), var(--yap-shadow-sm);
    color: var(--yap-muted);
    cursor: pointer;
    opacity: 0;
    transition: opacity var(--yap-dur);
  }
  .fx::before {
    content: '';
    position: absolute;
    inset: -3px;
    border-radius: 50%;
  }
  .fx:hover {
    color: var(--yap-fg);
  }
  .fx svg {
    width: 9px;
    height: 9px;
  }
  .filechip:hover .fx,
  .filechip:focus-within .fx {
    opacity: 1;
  }
  @media (hover: none), (pointer: coarse) {
    .fx {
      opacity: 1;
    }
  }
</style>
