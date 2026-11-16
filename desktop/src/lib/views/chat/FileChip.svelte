<script>
  // **A file chip** (16 Nov 2026, the Claude app's file chips at its own sizes: fileChip.js): one compact box — a
  // small type badge coloured by the file's family ("MD", "PDF", "XLSX", "CSV"…; app.css `--yap-file-*`, light and
  // dark), the file's name (a long one cut in the middle, so its ending shows), its size quiet beside it. A click
  // opens it. Every file Chat shows as a box is one:
  // - **a file attached to a question** (chat/attachments.rs, 15 Nov 2026): in Chat's box with ✕; on a sent question
  //   it opens the file (Markdown in Yap's own window, anything else in its app; a spreadsheet beside the chat;
  //   what could run a program shows its text). Past "Keep chat files": greyed, "removed". A PDF whose scanned
  //   pages were read with OCR says so ("OCR", its tooltip how many). While Rust still reads one (`reading`: a
  //   scan's OCR takes a few seconds), "Reading…".
  // - **a file shared into the conversation, or an agent's report's** (SharedFiles.svelte, `onopen`/`onmenu`:
  //   sharedOpen.js): a click opens it beside the chat in the view that suits it; a **⋯** on hover or keyboard
  //   focus (and its right-click, the ContextMenu key, Shift+F10) opens its menu — Open in Yap, Open in <editor>
  //   or its app, Show in folder, Copy path. A file no longer at its path says "Moved or deleted".
  import { invoke } from '@tauri-apps/api/core';
  import { getContext } from 'svelte';
  import { SHEETS } from './sheetView.js';
  import { toast } from '../../ui/toast.svelte.js';
  import { menuKey, menuOpenFor } from '../../ui/contextMenu.svelte.js';
  import { fileSize, fileTitle, ocrLine } from './chatFiles.js';
  import { badgeOf, familyOf, nameParts } from './fileChip.js';

  /**
   * `file`: an Attachment ({ id, name, kind, bytes, tokens, pages?, ocrPages?, unreadPages?, gone? }), `{ name,
   * reading: true }` while it's read, or a shared file ({ name, path, kind, bytes, missing? }); `onremove`: shown in
   * the box (✕). `onopen()`: what a click does instead of opening an attachment; `onmenu(e, el)`: its ⋯ and its
   * right-click; `onlook()`: the pointer or the focus came to it (its menu's facts asked for ahead).
   */
  let { file, onremove = null, onopen = null, onmenu = null, onlook = null, title = '', testid = 'chat-file-chip' } = $props();

  const badge = $derived(badgeOf(file));
  const family = $derived(familyOf(file));
  const parts = $derived(nameParts(file.name));
  const ocr = $derived(file.reading ? '' : ocrLine(file));
  const off = $derived(!!(file.gone || file.missing));
  const size = $derived(file.missing ? 'Moved or deleted' : file.gone ? 'removed' : fileSize(file.bytes));
  let opening = $state(false);
  let el = $state(null);
  // A spreadsheet opens beside the chat (SheetTab), where Chat has the panel.
  const sheets = getContext(SHEETS) ?? null;

  async function open() {
    if (off || file.reading || opening) return;
    if (onopen) {
      opening = true;
      try {
        await onopen();
      } finally {
        opening = false;
      }
      return;
    }
    if (file.kind === 'sheet' && sheets) {
      sheets.open(file);
      return;
    }
    opening = true;
    try {
      await invoke('chat_attachment_open', { id: file.id });
    } catch (e) {
      toast({ title: `Couldn't open ${file.name}`, description: String(e?.message ?? e), variant: 'destructive' });
    } finally {
      opening = false;
    }
  }

  const menu = (e) => onmenu?.(e, el);
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<span
  bind:this={el}
  class="filechip fam-{family}"
  class:gone={off}
  class:reading={file.reading}
  class:inbox={!!onremove}
  class:hasmenu={!!onmenu}
  class:menuopen={!!onmenu && menuOpenFor(el)}
  data-testid={file.reading ? 'chat-file-reading' : testid}
  data-kind={file.kind}
  data-family={family}
  oncontextmenu={onmenu ? menu : undefined}
  onpointerenter={onlook ? () => onlook() : undefined}
  onfocusin={onlook ? () => onlook() : undefined}
>
  {#if file.reading}
    <span class="fbody" role="status" aria-label={`Reading ${file.name}…`}>
      <span class="fbadge" aria-hidden="true">{badge}</span>
      <span class="fname"><span class="fhead">{parts.head}</span><span class="ftail">{parts.tail}</span></span>
      <span class="fsize">Reading…</span>
    </span>
  {:else}
    <button
      type="button"
      class="fbody"
      title={title || fileTitle(file)}
      disabled={off && !onmenu}
      aria-disabled={off ? 'true' : undefined}
      onclick={open}
      onkeydown={onmenu ? (e) => menuKey(e, menu) : undefined}
      aria-label={`${file.name}, ${size}${ocr ? `, ${ocr}` : ''}${off ? '' : '. Open it'}`}
      data-testid="chat-file-open"
    >
      <span class="fbadge" aria-hidden="true" data-testid="chat-file-badge">{badge}</span>
      <span class="fname" data-testid="chat-file-name"><span class="fhead">{parts.head}</span><span class="ftail">{parts.tail}</span></span>
      <span class="fsize" class:fgone={file.missing} data-testid={file.missing ? 'chat-file-gone' : 'chat-file-size'}>{size}</span>
      {#if ocr && !off}<span class="focr" data-testid="chat-file-ocr" aria-hidden="true">OCR</span>{/if}
    </button>
    {#if onmenu}
      <button type="button" class="fmore" data-more data-testid="chat-file-more" aria-label={`More for ${file.name}`} aria-haspopup="menu" title="More" onclick={menu}>
        <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><circle cx="5.5" cy="12" r="1.7" /><circle cx="12" cy="12" r="1.7" /><circle cx="18.5" cy="12" r="1.7" /></svg>
      </button>
    {/if}
  {/if}
  {#if onremove && !file.reading}
    <button type="button" class="fx" aria-label={`Remove ${file.name}`} title="Remove" onclick={() => onremove(file.id)}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" aria-hidden="true"><path d="M7 7l10 10M17 7 7 17" /></svg>
    </button>
  {/if}
</span>

<style>
  /* The Claude app's chip, measured on its screenshot (its answer text 15.7 px): a box 36 px tall with a 7 px
     corner, 8 px in from each side; the badge an 18 px pill with 10.5 px letters; the name 13.5 px; the size
     11.5 px — the app's own sizes at the default reading size, and in step with Settings → Appearance's reading
     size from there (as the rows under a message: ChatMessage.svelte `--row`). */
  .filechip {
    --s: var(--yap-reading-scale, 1);
    --fc: var(--yap-file-text);
    position: relative;
    display: inline-flex;
    align-items: center;
    box-sizing: border-box;
    max-width: min(100%, calc(340px * var(--s)));
    min-width: 0;
    border: 1px solid var(--yap-border-subtle);
    border-radius: calc(7px * var(--s));
    background: var(--yap-s2);
    color: var(--yap-fg);
    font-family: var(--yap-font-ui, inherit);
    vertical-align: middle;
    transition: border-color var(--yap-dur, 120ms);
  }
  .fam-doc {
    --fc: var(--yap-file-doc);
  }
  .fam-pdf {
    --fc: var(--yap-file-pdf);
  }
  .fam-sheet {
    --fc: var(--yap-file-sheet);
  }
  .fam-slides {
    --fc: var(--yap-file-slides);
  }
  .fam-picture {
    --fc: var(--yap-file-picture);
  }
  .fam-code {
    --fc: var(--yap-file-code);
  }
  .fam-archive {
    --fc: var(--yap-file-archive);
  }
  .fam-media {
    --fc: var(--yap-file-media);
  }
  .filechip:hover:not(.gone),
  .filechip.menuopen {
    border-color: var(--yap-border-hover, var(--yap-border));
  }
  .fbody {
    display: inline-flex;
    align-items: center;
    gap: calc(8px * var(--s));
    box-sizing: border-box;
    min-width: 0;
    max-width: 100%;
    min-height: calc(34px * var(--s));
    padding: 0 calc(9px * var(--s)) 0 calc(7px * var(--s));
    border: none;
    border-radius: inherit;
    background: none;
    color: inherit;
    font: inherit;
    font-size: calc(13.5px * var(--s));
    text-align: left;
    cursor: pointer;
  }
  .fbody:disabled,
  .gone .fbody {
    cursor: default;
  }
  .fbody:focus-visible,
  .fx:focus-visible,
  .fmore:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  /* The type: a small pill in its family's colour (its letters on a wash of it). */
  .fbadge {
    flex: 0 0 auto;
    box-sizing: border-box;
    min-width: calc(22px * var(--s));
    padding: 0 calc(4.5px * var(--s));
    border: 1px solid color-mix(in srgb, var(--fc) 34%, transparent);
    border-radius: var(--yap-r-full);
    background: color-mix(in srgb, var(--fc) 13%, transparent);
    color: var(--fc);
    font-size: calc(10.5px * var(--s));
    font-weight: 650;
    line-height: calc(16px * var(--s));
    letter-spacing: 0.03em;
    text-align: center;
  }
  /* The name, cut in the middle: its start gives way with "…", its ending stays whole. */
  .fname {
    display: inline-flex;
    min-width: 0;
    font-weight: 500;
    white-space: pre;
  }
  .fhead {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .ftail {
    flex: 0 0 auto;
  }
  .fsize {
    flex: 0 0 auto;
    color: var(--yap-muted);
    font-size: calc(11.5px * var(--s));
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
    transition: opacity var(--yap-dur, 120ms);
  }
  .fgone {
    font-style: italic;
  }
  /* Read with OCR: a small tag after the size (its tooltip says how many pages). */
  .focr {
    flex: 0 0 auto;
    padding: 0 4px;
    border-radius: 4px;
    background: color-mix(in srgb, var(--yap-fg) 8%, var(--yap-s2));
    color: var(--yap-muted);
    font-size: calc(9.5px * var(--s));
    font-weight: 600;
    line-height: 15px;
    letter-spacing: 0.03em;
  }
  .reading .fbody {
    cursor: default;
  }
  .reading .fsize {
    animation: fread 1.4s ease-in-out infinite;
  }
  @keyframes fread {
    50% {
      opacity: 0.45;
    }
  }
  .gone .fname {
    color: var(--yap-muted);
  }
  .gone .fbadge {
    opacity: 0.55;
  }
  /* ⋯: over the size, at the chip's end, on hover or keyboard focus (and while its menu is open): nothing moves.
     The size keeps room for it. */
  .hasmenu .fsize {
    min-width: calc(24px * var(--s));
    text-align: right;
  }
  .fmore {
    position: absolute;
    top: 50%;
    right: calc(5px * var(--s));
    display: grid;
    place-items: center;
    width: calc(24px * var(--s));
    height: calc(24px * var(--s));
    padding: 0;
    border: none;
    border-radius: calc(5px * var(--s));
    background: color-mix(in srgb, var(--yap-fg) 8%, var(--yap-s2));
    color: var(--yap-muted);
    cursor: pointer;
    opacity: 0;
    transform: translateY(-50%);
    transition: opacity var(--yap-dur, 120ms);
  }
  .fmore:hover {
    color: var(--yap-fg);
  }
  .fmore svg {
    width: calc(15px * var(--s));
    height: calc(15px * var(--s));
  }
  .filechip:hover .fmore,
  .filechip:focus-within .fmore,
  .filechip.menuopen .fmore {
    opacity: 1;
  }
  .filechip.hasmenu:hover .fsize,
  .filechip.hasmenu:focus-within .fsize,
  .filechip.menuopen .fsize {
    opacity: 0;
  }
  /* In the box: ✕ out over the corner on hover, as a screenshot tile's. */
  .inbox .fbody {
    padding-right: calc(10px * var(--s));
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
    .fx,
    .fmore {
      opacity: 1;
    }
    .filechip.hasmenu .fsize {
      opacity: 0;
    }
  }
</style>
