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
  // original. **Any other file is a chip** (16 Nov 2026, the Claude app's:
  // FileChip.svelte — its type badge, its name, its size), several in a
  // row that wraps: a click opens it beside the chat in the view that suits
  // it (sharedOpen.js: where a Code chat's links open for a file of its
  // workspace; else the file view, the sheet view, or its own app), its ⋯
  // its menu (Open in Yap, Open in <editor> or its app, Show in folder, Copy
  // path); a file no longer there says "Moved or deleted". Opening it is its
  // preview (no "▸ Preview" any more). **Each file once**: a file two shares
  // of one answer hold shows once, and an agent's report leaves out a file
  // whose one chip is elsewhere in the turn (`elsewhere`, fileChip.js
  // `chippedElsewhere`: the answer shares it). On an agent's report
  // (`report`): up to four pictures, then "+N more", and its new files as
  // chips, four then "+N more". Its own small chunk.
  import { invoke } from '@tauri-apps/api/core';
  import { getContext } from 'svelte';
  import { placed, shown, copyImage, saveImage, chatImageUrl } from './chatImages.js';
  import { screenPixels } from './chatShots.svelte.js';
  import { openContextMenu } from '../../ui/contextMenu.svelte.js';
  import { openExternalLink } from '../../core/externalLinks.js';
  import { toast } from '../../ui/toast.svelte.js';
  import { shareParts, gridColumns, asShownPicture, moreLabel, tileSize, coverPlaced, ONE_MAX_H, GRID_GAP } from './shares.js';
  import { onceEach } from './fileChip.js';
  import { SHARED_FILES, workspaceRef } from './sharedOpen.js';
  import FileChip from './FileChip.svelte';

  /**
   * `shares`: the message's `files` (or as they come, `liveFiles`);
   * `conversation`: the chat's id (Open, Show in folder);
   * `room`: the readable column's width (CSS px); `onpicture(p)`: a picture
   * opened (a `ShownPicture`); `report`: an agent's report card's;
   * `elsewhere`: the files (normalised paths) whose one chip is elsewhere in
   * the turn — left out here.
   */
  let { shares = [], conversation = null, room = 0, onpicture = null, report = false, elsewhere = null } = $props();

  // Chat's way to open a shared file beside the chat, and its menu (ChatView.svelte; none: its app, as before).
  const files = getContext(SHARED_FILES) ?? null;

  /** The width a share may take (CSS px). */
  const width = $derived(Math.max(160, report ? Math.min(room || 520, 520) : room || 640));

  /** Each share as it shows: its pictures and its chips, each file once in the message. */
  const parts = $derived.by(() => {
    const seen = new Set();
    return (shares || []).map((share) => {
      const p = shareParts(share, { report });
      return { ...p, pictures: onceEach(p.pictures, { skip: elsewhere, seen }), cards: onceEach(p.cards, { skip: elsewhere, seen }) };
    });
  });

  /** A grid's tile side (CSS px) for `n` pictures. */
  function tile(n) {
    const cols = gridColumns(n);
    return Math.max(80, Math.floor((width - GRID_GAP * (cols - 1)) / cols));
  }

  function failed(what, e) {
    toast({ title: what, description: String(e?.message ?? e ?? ''), variant: 'destructive' });
  }

  /** Its own app (a picture's Open original; a chip's click with no Chat around it). */
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

  /** A chip's click: beside the chat (sharedOpen.js), else its app. */
  const openChip = (item) => (files ? files.open(item) : item.opens ? openFile(item) : reveal(item));
  /** A chip's ⋯ and right-click. */
  const chipMenu = (e, item, el) => files?.menu(e, item, el);
  /** The pointer or the focus on a chip: whether it's a workspace's file, asked ahead of its menu. */
  const look = (item) => files?.code && workspaceRef(files.conversation, item.path, true);

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
</script>

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

{#each shares as share, si (si)}
  {@const p = parts[si]}
  {#if p && (p.pictures.length || p.cards.length || p.morePictures || p.moreFiles)}
    <figure class="share" class:report style:--share-width="{width}px" data-testid="chat-share" aria-label={share.caption || 'Shared files'}>
      {#if p.pictures.length === 1 && !report && !p.morePictures}
        <!-- One: whole, never cropped, its frame its own shape (no bars). -->
        {@render picture(p.pictures[0], placed(p.pictures[0], { maxW: width, maxH: ONE_MAX_H, ratio: screenPixels.ratio }), share.caption, true)}
      {:else if p.pictures.length || p.morePictures}
        {@const n = p.pictures.length + (p.morePictures ? 1 : 0)}
        {@const size = tileSize(tile(n))}
        {@const frame = coverPlaced({ w: 0, h: 0 }, size.w, size.h, screenPixels.ratio, '').frame}
        <div class="grid" style:grid-template-columns="repeat({gridColumns(n)}, {frame.w}px)" data-testid="chat-shared-grid">
          {#each p.pictures as item, i (`${item.id}:${i}`)}
            {@render picture(item, { ...coverPlaced(item, size.w, size.h, screenPixels.ratio, chatImageUrl(item.id)), cover: true }, share.caption, false)}
          {/each}
          {#if p.morePictures}
            <span class="moretile" style:width="{frame.w}px" style:height="{frame.h}px" data-testid="chat-shared-more">{moreLabel(p.morePictures)}</span>
          {/if}
        </div>
      {/if}
      {#if p.cards.length || p.moreFiles}
        <!-- Any other file: a chip each, in a row that wraps. -->
        <div class="chips" data-testid="chat-file-chips">
          {#each p.cards as item, i (`${item.path || item.id}:${i}`)}
            <FileChip
              file={item}
              title={`${item.path || item.name}${item.pages > 0 ? ` · ${item.pages === 1 ? '1 page' : `${item.pages} pages`}` : ''}`}
              testid="chat-file-card"
              onopen={() => openChip(item)}
              onmenu={item.path ? (e, el) => chipMenu(e, item, el) : null}
              onlook={files?.code && item.path ? () => look(item) : null}
            />
          {/each}
          {#if p.moreFiles}
            <span class="morefiles" data-testid="chat-shared-more-files">{moreLabel(p.moreFiles)}</span>
          {/if}
        </div>
      {/if}
      <!-- A caption that only repeats the one file's name says nothing new; a quiet line under the chips. -->
      {#if share.caption && !(share.items.length === 1 && share.caption.trim() === share.items[0].name)}
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
  /* The chips: side by side, wrapping onto the next line as the column runs out. */
  .chips {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    max-width: min(var(--share-width), 100%);
  }
  .morefiles {
    font-family: var(--yap-font-ui, inherit);
    font-size: calc(11.5px * var(--yap-reading-scale, 1));
    color: var(--yap-muted);
    padding-left: 2px;
  }
</style>
