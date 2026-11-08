// Files shared into the conversation (`share_files`,
// desktop/src-tauri/src/chat/shares.rs): how a share is laid out in the
// thread — one picture large, several as a grid, any other file as a card —
// and the words on its cards. Pure, no Svelte state (SharedFiles.svelte
// draws it; an agent's report card shows its turn's files the same way).

/** A single picture: at most this tall (CSS px), as wide as the readable column. */
export const ONE_MAX_H = 480;
/** The gap between a grid's tiles (CSS px). */
export const GRID_GAP = 6;
/** An agent's report card: pictures and other files it shows, at most. */
export const REPORT_MAX = 4;

/** A grid's tiles: this shape (width over height), each filled with its picture's middle. */
export const TILE_ASPECT = 4 / 3;

/** A tile `side` CSS px wide: its CSS size. */
export function tileSize(side) {
  return { w: side, h: Math.max(1, Math.round(side / TILE_ASPECT)) };
}

/**
 * The part of a `iw` × `ih` picture a `w` × `h` tile filled with it shows
 * (CSS's `object-fit: cover`): `[x, y, width, height]`, centred, in the
 * picture's pixels (snip/thumbs.rs `cover_crop`, the same sum).
 */
export function coverCrop(iw, ih, w, h) {
  if (!(iw > 0 && ih > 0 && w > 0 && h > 0)) return [0, 0, iw || 0, ih || 0];
  if (iw * h > ih * w) {
    const cw = Math.min(iw, Math.max(1, Math.round((ih * w) / h)));
    return [Math.floor((iw - cw) / 2), 0, cw, ih];
  }
  const ch = Math.min(ih, Math.max(1, Math.round((iw * h) / w)));
  return [0, Math.floor((ih - ch) / 2), iw, ch];
}

/**
 * **A tile filled with picture `img`** ({ w, h }), `w` × `h` CSS px at
 * `ratio` device pixels per CSS px, `url` the picture's address: the
 * frame (CSS px, on whole device pixels) and its source — Rust's tile at
 * exactly those device pixels (`?w=&h=&fit=cover`, snip/thumbs.rs) when
 * the picture's middle is at least that big, else the picture itself, the
 * browser filling the tile (`exact` false: a picture smaller than its tile).
 */
export function coverPlaced(img, w, h, ratio, url) {
  const r = ratio > 0 ? ratio : 1;
  const dw = Math.max(1, Math.round(w * r));
  const dh = Math.max(1, Math.round(h * r));
  const frame = { w: dw / r, h: dh / r };
  if (!(img?.w > 0 && img?.h > 0)) return { frame, src: url, exact: false };
  if (img.w === dw && img.h === dh) return { frame, src: url, exact: true };
  const [, , cw, ch] = coverCrop(img.w, img.h, dw, dh);
  const exact = dw <= cw && dh <= ch;
  return { frame, src: exact ? `${url}?w=${dw}&h=${dh}&fit=cover` : url, exact };
}

/** A grid's columns for `n` pictures: 2, and 3 from 5. */
export function gridColumns(n) {
  return n >= 5 ? 3 : 2;
}

/**
 * A share's parts as they show: its pictures (copied in, still there) and
 * its cards (every other file, and a picture whose copy is gone), with how
 * many more there were (an agent's report: `report`, at most
 * {@link REPORT_MAX} of each).
 */
export function shareParts(share, { report = false } = {}) {
  const items = Array.isArray(share?.items) ? share.items : [];
  let pictures = items.filter((i) => i?.id && !i.gone);
  let cards = items.filter((i) => i && !(i.id && !i.gone));
  let morePictures = Math.max(0, Number(share?.morePictures) || 0);
  let moreFiles = Math.max(0, Number(share?.moreFiles) || 0);
  if (report) {
    morePictures += Math.max(0, pictures.length - REPORT_MAX);
    moreFiles += Math.max(0, cards.length - REPORT_MAX);
    pictures = pictures.slice(0, REPORT_MAX);
    cards = cards.slice(0, REPORT_MAX);
  }
  return { pictures, cards, morePictures, moreFiles };
}

/** "340 B", "12 KB", "1.4 MB", "2.1 GB". */
export function sizeLabel(bytes) {
  const n = Math.max(0, Number(bytes) || 0);
  if (n < 1024) return `${n} B`;
  const units = ['KB', 'MB', 'GB', 'TB'];
  let v = n / 1024;
  let u = 0;
  while (v >= 1024 && u < units.length - 1) {
    v /= 1024;
    u += 1;
  }
  return `${v >= 10 || u === 0 ? Math.round(v) : v.toFixed(1)} ${units[u]}`;
}

/** A folder, shortened to its last two parts ("…\garden-planner\docs"); the whole when it's short. */
export function shortFolder(folder) {
  const f = String(folder ?? '');
  const sep = f.includes('\\') ? '\\' : '/';
  const parts = f.split(/[\\/]/).filter(Boolean);
  if (parts.length <= 3) return f;
  return `…${sep}${parts.slice(-2).join(sep)}`;
}

/** What a card says a file is: "Markdown", "PDF · 3 pages", "Picture"… */
export function kindLabel(item) {
  switch (item?.kind) {
    case 'markdown':
      return 'Markdown';
    case 'text':
      return 'Text';
    case 'code':
      return 'Code';
    case 'pdf':
      return item.pages > 0 ? `PDF · ${item.pages === 1 ? '1 page' : `${item.pages} pages`}` : 'PDF';
    case 'image':
      return 'Picture';
    default:
      return 'File';
  }
}

/** Whether a file's card has a preview to unfold (shares.rs `previews`). */
export function hasPreview(item) {
  return ['markdown', 'text', 'code'].includes(item?.kind) && !!item?.path && !item?.missing;
}

/** A folder's own name ("docs"). */
function folderName(folder) {
  const parts = String(folder ?? '').split(/[\\/]/).filter(Boolean);
  return parts.at(-1) ?? '';
}

/**
 * A shared picture as the panel's picture tab and the viewer take it (a
 * `ShownPicture`, chat/pictures.rs): its file's name as its caption, its
 * folder as where it's from (no page to open), or its page from the web.
 */
export function asShownPicture(item, caption = '') {
  const page = item?.page?.title || item?.page?.url ? item.page : { title: item?.folder || '', site: folderName(item?.folder), url: '' };
  return { id: item?.id, w: item?.w || 0, h: item?.h || 0, caption: item?.name || 'Picture', alt: caption || '', imageUrl: item?.imageUrl || '', page, n: 0 };
}

/** "+3 more", for what a report card doesn't show. */
export function moreLabel(n) {
  return n > 0 ? `+${n} more` : '';
}
