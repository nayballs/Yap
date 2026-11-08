<script>
  // **Files shared into the conversation** (`share_files`,
  // desktop/src-tauri/src/chat/shares.rs; after the Claude app's
  // SendUserFile): each share under the answer that made it — one picture
  // large and whole (the readable column's width, at most 480 px tall, its
  // shape kept: chatImages.js `placed`), several as a grid (2 columns, 3
  // from five) of 4:3 tiles each filled with its picture's middle
  // (shares.js `coverPlaced`: Rust's tile at exactly its device pixels,
  // `?fit=cover`), the caption under them; a click opens the whole picture in the panel's
  // picture tab, or the viewer while the panel is closed (ChatThread). A
  // picture's ⋯ (and its right-click): Copy / Save / Show in folder / Open
  // original. Any other file is a card: an icon by its kind, its name, its
  // folder (shortened; the whole on hover), size and kind, the first lines
  // folded under ▸ (read from the file when unfolded: `chat_file_preview`),
  // and Open (a Markdown file in Yap's own window, any other in its app — on
  // the person's click only), Show in folder, Copy path; a file no longer
  // there says "Moved or deleted". On an agent's report (`report`): up to
  // four pictures, then "+N more", and its new files as compact cards. Its
  // own small chunk.
  import { invoke } from '@tauri-apps/api/core';
  import { placed, shown, copyImage, saveImage, chatImageUrl } from './chatImages.js';
  import { screenPixels } from './chatShots.svelte.js';
  import { openContextMenu } from '../../ui/contextMenu.svelte.js';
  import { copyText } from '../../core/itemMenus.js';
  import { openExternalLink } from '../../core/externalLinks.js';
  import { toast } from '../../ui/toast.svelte.js';
  import { shareParts, gridColumns, sizeLabel, shortFolder, kindLabel, hasPreview, asShownPicture, moreLabel, tileSize, coverPlaced, ONE_MAX_H, GRID_GAP } from './shares.js';

  /**
   * `shares`: the message's `files` (or as they come, `liveFiles`);
   * `conversation`: the chat's id (Open, Show in folder, a preview);
   * `room`: the readable column's width (CSS px); `onpicture(p)`: a picture
   * opened (a `ShownPicture`); `report`: an agent's report card's.
   */
  let { shares = [], conversation = null, room = 0, onpicture = null, report = false } = $props();

  /** The width a share may take (CSS px). */
  const width = $derived(Math.max(160, report ? Math.min(room || 520, 520) : room || 640));

  /** A grid's tile side (CSS px) for `n` pictures. */
  function tile(n) {
    const cols = gridColumns(n);
    return Math.max(80, Math.floor((width - GRID_GAP * (cols - 1)) / cols));
  }

  function failed(what, e) {
    toast({ title: what, description: String(e?.message ?? e ?? ''), variant: 'destructive' });
  }

  async function openFile(item) {
    if (conversation == null || !item?.path) return;
    try {
      await invoke('chat_file_open', { conversation, path: item.path });
    } catch (e) {
      failed(`Couldn't open ${item.name}`, e);
    }
  }

  async function reveal(item) {
    if (conversation == null || !item?.path) return;
    try {
      await invoke('chat_file_reveal', { conversation, path: item.path });
    } catch (e) {
      failed("Couldn't show it in its folder", e);
    }
  }

  const copyPath = (item) => copyText(item.path, 'Path copied');

  /** A picture's menu: Copy, Save, Show in folder, Open original. */
  function pictureItems(item) {
    const fromWeb = !item.path && !!item.imageUrl;
    return [
      { label: 'Copy image', icon: 'copy', onselect: () => copyImage(item.id, 'Picture') },
      { label: 'Save image as…', icon: 'download', onselect: () => saveImage(item.id, 'Picture') },
      { separator: true },
      { label: 'Show in folder', icon: 'folder', onselect: () => reveal(item), disabled: !item.path || item.missing },
      { label: 'Open original', icon: 'open', onselect: () => (fromWeb ? openExternalLink(item.imageUrl) : openFile(item)), disabled: fromWeb ? false : !item.path || item.missing || !item.opens },
    ];
  }

  function pictureMenu(e, item) {
    e.preventDefault();
    e.stopPropagation();
    openContextMenu(e, pictureItems(item), { label: 'Picture' });
  }

  // Each card's preview: read once, when it's first unfolded.
  let previews = $state({});
  async function loadPreview(e, item) {
    if (!e.currentTarget.open || previews[item.path] !== undefined || conversation == null) return;
    previews = { ...previews, [item.path]: { loading: true } };
    try {
      const p = await invoke('chat_file_preview', { conversation, path: item.path });
      previews = { ...previews, [item.path]: p ? { text: p.text, cut: p.cut } : { none: true } };
    } catch {
      previews = { ...previews, [item.path]: { none: true } };
    }
  }
</script>

{#snippet kindIcon(kind)}
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    {#if kind === 'image'}
      <rect x="3" y="4" width="18" height="16" rx="2" /><circle cx="9" cy="10" r="1.6" /><path d="m21 16-5-5-9 9" />
    {:else if kind === 'pdf'}
      <path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z" /><path d="M14 3v5h5" /><path d="M8.5 16.5h7M8.5 13h4" />
    {:else if kind === 'code'}
      <path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z" /><path d="M14 3v5h5" /><path d="m10 12-2 2 2 2M14 12l2 2-2 2" />
    {:else if kind === 'markdown' || kind === 'text'}
      <path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z" /><path d="M14 3v5h5" /><path d="M9 13h6M9 17h4" />
    {:else}
      <path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z" /><path d="M14 3v5h5" />
    {/if}
  </svg>
{/snippet}

{#snippet picture(item, p, caption, one)}
  <!-- `p`: where it goes — the whole picture (chatImages.js `placed`), or a tile filled with its middle (`cover`). -->
  <div class="shotframe sharedpic" class:one class:tile={p.cover} style:width="{p.frame.w}px" style:aspect-ratio="{p.frame.w} / {p.frame.h}">
    <button
      type="button"
      class="picbtn"
      data-testid="chat-shared-picture"
      title={item.path || item.name}
      aria-label={`Open the picture ${item.name}`}
      onclick={() => onpicture?.(asShownPicture(item, caption))}
      oncontextmenu={(e) => pictureMenu(e, item)}
    >
      {#if p.cover}
        <img class="cover" src={p.src} alt={caption || item.name} draggable="false" decoding="async" use:shown />
      {:else}
        <img src={p.src} alt={caption || item.name} draggable="false" decoding="async" use:shown style:left={p.pic.x} style:top={p.pic.y} style:width={p.pic.w} style:height={p.pic.h} />
      {/if}
    </button>
    {#if one}
      <button type="button" class="picmore" data-more data-testid="chat-shared-picture-more" aria-label={`More for ${item.name}`} title="More" onclick={(e) => pictureMenu(e, item)}>
        <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><circle cx="5.5" cy="12" r="1.6" /><circle cx="12" cy="12" r="1.6" /><circle cx="18.5" cy="12" r="1.6" /></svg>
      </button>
    {/if}
  </div>
{/snippet}

{#snippet card(item, compact)}
  {@const gone = item.missing || (item.gone && !item.path)}
  {@const pv = previews[item.path]}
  <div class="filecard kind-{item.kind || 'file'}" class:compact class:missing={gone} data-testid="chat-file-card" data-kind={item.kind}>
    <div class="frow">
      <span class="ficon">{@render kindIcon(item.kind)}</span>
      <div class="fmain">
        <span class="fname" title={item.path || item.name} data-testid="chat-file-name">{item.name}</span>
        {#if gone}
          <span class="fmeta fgone" data-testid="chat-file-gone">Moved or deleted</span>
        {:else}
          <span class="fmeta">
            {#if item.folder}<span class="ffolder" title={item.folder}>{shortFolder(item.folder)}</span>{' · '}{/if}{sizeLabel(item.bytes)}{' · '}{kindLabel(item)}
          </span>
        {/if}
      </div>
      <div class="factions">
        {#if !gone && item.opens}
          <button type="button" class="fbtn primary" data-testid="chat-file-open" onclick={() => openFile(item)} aria-label={`Open ${item.name}`}>Open</button>
        {/if}
        {#if !gone && item.path}
          <button type="button" class="fbtn" data-testid="chat-file-reveal" onclick={() => reveal(item)} aria-label={`Show ${item.name} in its folder`} title="Show in folder">
            {#if compact}
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" /></svg>
            {:else}Show in folder{/if}
          </button>
        {/if}
        {#if item.path && !compact}
          <button type="button" class="fbtn" data-testid="chat-file-copy-path" onclick={() => copyPath(item)} aria-label={`Copy the path of ${item.name}`}>Copy path</button>
        {/if}
      </div>
    </div>
    {#if !compact && hasPreview(item)}
      <details class="fpreview" data-testid="chat-file-preview" ontoggle={(e) => loadPreview(e, item)}>
        <summary>Preview</summary>
        {#if pv?.text != null}
          <pre data-testid="chat-file-preview-text">{pv.text}{#if pv.cut}{'\n…'}{/if}</pre>
        {:else if pv?.none}
          <p class="fnote">Nothing to preview.</p>
        {:else}
          <p class="fnote">Reading…</p>
        {/if}
      </details>
    {/if}
  </div>
{/snippet}

{#each shares as share, si (si)}
  {@const parts = shareParts(share, { report })}
  {#if parts.pictures.length || parts.cards.length || parts.morePictures || parts.moreFiles}
    <figure class="share" class:report style:--share-width="{width}px" data-testid="chat-share" aria-label={share.caption || 'Shared files'}>
      {#if parts.pictures.length === 1 && !report}
        <!-- One: whole, never cropped, its frame its own shape (no bars). -->
        {@render picture(parts.pictures[0], placed(parts.pictures[0], { maxW: width, maxH: ONE_MAX_H, ratio: screenPixels.ratio }), share.caption, true)}
      {:else if parts.pictures.length || parts.morePictures}
        {@const n = parts.pictures.length + (parts.morePictures ? 1 : 0)}
        {@const size = tileSize(tile(n))}
        {@const frame = coverPlaced({ w: 0, h: 0 }, size.w, size.h, screenPixels.ratio, '').frame}
        <div class="grid" style:grid-template-columns="repeat({gridColumns(n)}, {frame.w}px)" data-testid="chat-shared-grid">
          {#each parts.pictures as item, i (`${item.id}:${i}`)}
            {@render picture(item, { ...coverPlaced(item, size.w, size.h, screenPixels.ratio, chatImageUrl(item.id)), cover: true }, share.caption, false)}
          {/each}
          {#if parts.morePictures}
            <span class="moretile" style:width="{frame.w}px" style:height="{frame.h}px" data-testid="chat-shared-more">{moreLabel(parts.morePictures)}</span>
          {/if}
        </div>
      {/if}
      {#if parts.cards.length}
        <div class="cards">
          {#each parts.cards as item, i (`${item.path || item.id}:${i}`)}
            {@render card(item, report)}
          {/each}
          {#if parts.moreFiles}
            <span class="morefiles" data-testid="chat-shared-more-files">{moreLabel(parts.moreFiles)}</span>
          {/if}
        </div>
      {/if}
      {#if share.caption}
        <figcaption class="caption" data-testid="chat-share-caption">{share.caption}</figcaption>
      {/if}
    </figure>
  {/if}
{/each}

<style>
  .share {
    align-self: flex-start;
    display: flex;
    flex-direction: column;
    gap: 6px;
    box-sizing: border-box;
    width: fit-content;
    max-width: min(var(--share-width), 100%);
    margin: 0;
  }
  .share.report {
    margin-top: -4px;
  }
  .grid {
    display: grid;
    gap: 6px;
    max-width: 100%;
  }
  /* A picture's button fills its frame (app.css .shotframe). */
  .sharedpic {
    max-width: 100%;
  }
  /* A grid's tile: filled with its picture's middle; while it loads, the
     quiet surface (never the frame's white or a bright block). */
  .sharedpic.tile {
    background: var(--yap-s2);
  }
  .sharedpic img.cover {
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .picbtn {
    position: absolute;
    inset: 0;
    display: block;
    padding: 0;
    border: none;
    border-radius: inherit;
    overflow: hidden;
    background: none;
    cursor: zoom-in;
  }
  .picbtn:focus-visible {
    outline: none;
  }
  .sharedpic:has(.picbtn:focus-visible) {
    box-shadow: var(--yap-focus);
  }
  .picmore {
    position: absolute;
    top: 8px;
    right: 8px;
    z-index: 2;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-full);
    background: var(--yap-s1);
    color: var(--yap-fg);
    cursor: pointer;
    opacity: 0;
    transition: opacity 120ms ease;
  }
  .sharedpic:hover .picmore,
  .picmore:focus-visible {
    opacity: 1;
  }
  .picmore:focus-visible {
    outline: 2px solid var(--yap-primary-line);
    outline-offset: 1px;
  }
  .picmore svg {
    width: 15px;
    height: 15px;
  }
  .moretile {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    box-sizing: border-box;
    border: 1px dashed var(--yap-border);
    border-radius: 10px;
    background: var(--yap-s2);
    color: var(--yap-muted);
    font-family: var(--yap-font-ui, inherit);
    font-size: 13px;
    font-weight: 600;
  }
  .caption {
    font-family: var(--yap-font-ui, inherit);
    font-size: 11.5px;
    line-height: 1.45;
    color: var(--yap-muted);
    overflow-wrap: anywhere;
  }
  .cards {
    display: flex;
    flex-direction: column;
    gap: 6px;
    width: min(var(--share-width), 520px);
    max-width: 100%;
  }
  .morefiles {
    font-family: var(--yap-font-ui, inherit);
    font-size: 11.5px;
    color: var(--yap-muted);
    padding-left: 2px;
  }
  .filecard {
    box-sizing: border-box;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    background: var(--yap-s1);
    color: var(--yap-fg);
    font-family: var(--yap-font-ui, inherit);
    font-size: 12px;
  }
  .frow {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
    padding: 8px 10px;
  }
  .filecard.compact .frow {
    padding: 6px 8px;
    gap: 8px;
  }
  .ficon {
    flex: 0 0 auto;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    border-radius: 8px;
    background: var(--yap-primary-wash);
    color: var(--yap-primary);
  }
  .filecard.compact .ficon {
    width: 26px;
    height: 26px;
  }
  .ficon svg {
    width: 17px;
    height: 17px;
  }
  .fmain {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .fname {
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .fmeta {
    font-size: 11px;
    color: var(--yap-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .fgone {
    color: var(--yap-muted);
    font-style: italic;
  }
  .filecard.missing .ficon {
    background: var(--yap-s2);
    color: var(--yap-muted);
  }
  .filecard.missing .fname {
    color: var(--yap-muted);
  }
  .factions {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .fbtn {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 26px;
    padding: 0 9px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-full);
    background: var(--yap-s2);
    color: var(--yap-fg);
    font: inherit;
    font-size: 11.5px;
    font-weight: 600;
    white-space: nowrap;
    cursor: pointer;
  }
  .fbtn:hover {
    border-color: var(--yap-border-hover);
  }
  .fbtn:focus-visible {
    outline: 2px solid var(--yap-primary-line);
    outline-offset: 1px;
  }
  .fbtn.primary {
    color: var(--yap-primary);
  }
  .filecard.compact .fbtn {
    height: 24px;
    padding: 0 7px;
  }
  .fbtn svg {
    width: 13px;
    height: 13px;
  }
  .fpreview {
    border-top: 1px solid var(--yap-border-subtle);
    padding: 4px 10px 8px;
    color: var(--yap-muted);
    font-size: 11.5px;
  }
  .fpreview summary {
    width: fit-content;
    cursor: pointer;
    padding: 2px 0;
  }
  .fpreview summary:focus-visible {
    outline: 2px solid var(--yap-primary-line);
    outline-offset: 1px;
    border-radius: 4px;
  }
  .fpreview pre {
    max-height: 260px;
    overflow: auto;
    margin: 6px 0 0;
    padding: 8px 10px;
    border-radius: var(--yap-r-sm, 6px);
    background: var(--yap-s2);
    color: var(--yap-fg);
    font-family: ui-monospace, Consolas, monospace;
    font-size: 11px;
    line-height: 1.5;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .fnote {
    margin: 6px 0 0;
  }
</style>
