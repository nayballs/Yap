// Chat's screenshots (Snip to Chat, desktop/src-tauri/src/snip.rs): their address
// for an <img> — by id only: the yapsnip scheme serves
// <data>/chat-images/<id>.png and nothing else — what Enter asks, how
// a removed one is said, a screenshot's right-click menu (Copy image, Save
// image as…: snip/copy_save.rs), a picture pasted or dropped into the box
// (snip/attach.rs), and a question whose screenshot went as its text
// (snip/ocr.rs: the text after SCREEN_TEXT_MARK, folded in its bubble).
import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { toast } from '../../ui/toast.svelte.js';

/** What Enter on an empty box sends with a screenshot. */
export const DEFAULT_PROMPT = "What's this?";

/** Screenshots one message carries, at most (snip/images.rs `MAX_PER_REQUEST`: as many as Claude takes in one request; 4 before 14 Nov 2026). */
export const MAX_SHOTS = 100;

/** A screenshot's address (`http://yapsnip.localhost/<id>` in WebView2). */
export function chatImageUrl(id) {
  return convertFileSrc(id, 'yapsnip');
}

// ---- How a screenshot is drawn (after the Claude app's: snip.md "How a
// screenshot looks"). The picture sits whole in a frame — a fill under it,
// a ring one device pixel wide over its edge (app.css `--yap-shot-*`,
// `--yap-hairline`) — at exactly the device pixels it's drawn at: Rust makes
// that size (snip/thumbs.rs, `?w=&h=`), or it's the picture's own pixels,
// one per device pixel, when it fits. The browser never scales it.

/** The message box's tile: a square this many CSS px (the Claude app's 7.5rem; snip/thumbs.rs `TILE_CSS`). */
export const TILE = 120;
/** A picture in a question: at most this wide and tall (CSS px)… */
export const SENT_MAX_W = 480;
export const SENT_MAX_H = 320;
/** …each of several at most this wide… */
export const SENT_MAX_W_MANY = 240;
/** …and its frame never smaller than a tile, so a small one is still a thing to click. */
export const SENT_MIN = TILE;

/** `w` × `h` fitted inside `boxW` × `boxH`, its shape kept, never bigger, never under a pixel (snip/thumbs.rs `contain`). */
export function contain(w, h, boxW, boxH) {
  if (!(w > 0 && h > 0)) return [Math.max(1, w || 0), Math.max(1, h || 0)];
  const s = Math.min(boxW / w, boxH / h, 1);
  return [Math.max(1, Math.round(w * s)), Math.max(1, Math.round(h * s))];
}

/**
 * Where a picture `img` ({ id, w, h }) goes: a frame at most `maxW` × `maxH`
 * CSS px and at least `minW` × `minH`, at `ratio` device pixels per CSS px.
 * `{ frame: { w, h }, pic: { x, y, w, h }, src, thumb }` — the frame in CSS
 * px, the picture in it as % (it scales with the frame if a narrow window
 * squeezes it), on whole device pixels, centred; `src` the thumbnail at the
 * picture's device pixels, or the picture itself when that's its own size.
 */
export function placed(img, { maxW, maxH, minW = 0, minH = 0, ratio = 1 }) {
  const r = ratio > 0 ? ratio : 1;
  if (!(img?.w > 0 && img?.h > 0)) {
    // Its size not known (never seen, but chats.json is old data): the
    // picture itself, contained in the largest frame.
    return { frame: { w: maxW, h: maxH }, pic: { x: '0%', y: '0%', w: '100%', h: '100%' }, src: chatImageUrl(img?.id), thumb: false };
  }
  const dev = (css) => Math.max(1, Math.round(css * r));
  const [dw, dh] = contain(img.w, img.h, dev(maxW), dev(maxH));
  const fw = Math.max(dw, Math.min(dev(minW), dev(maxW)));
  const fh = Math.max(dh, Math.min(dev(minH), dev(maxH)));
  const ox = Math.floor((fw - dw) / 2);
  const oy = Math.floor((fh - dh) / 2);
  const pct = (n, of) => `${(n / of) * 100}%`;
  const own = dw === img.w && dh === img.h;
  return {
    frame: { w: fw / r, h: fh / r },
    pic: { x: pct(ox, fw), y: pct(oy, fh), w: pct(dw, fw), h: pct(dh, fh) },
    src: own ? chatImageUrl(img.id) : `${chatImageUrl(img.id)}?w=${dw}&h=${dh}`,
    thumb: !own,
  };
}

/** The box's tile for a waiting screenshot. */
export function tilePlaced(img, ratio) {
  return placed(img, { maxW: TILE, maxH: TILE, minW: TILE, minH: TILE, ratio });
}

/**
 * A question's picture: as wide as its own pixels at this ratio (one per
 * device pixel), at most 480 × 320 — 240 wide each when there are several,
 * and never wider than `room` (the thread's, CSS px) — in a frame at least
 * a tile's size.
 */
export function sentPlaced(img, ratio, room = 0, many = false) {
  let maxW = many ? SENT_MAX_W_MANY : SENT_MAX_W;
  if (room > 0) maxW = Math.max(SENT_MIN, Math.min(maxW, Math.floor(many ? (room - 6) / 2 : room)));
  return placed(img, { maxW, maxH: SENT_MAX_H, minW: SENT_MIN, minH: SENT_MIN, ratio });
}

/**
 * The viewer: the whole picture at one image pixel per device pixel when it
 * fits the window's `roomW` × `roomH` CSS px, else as big as fits.
 */
export function openPlaced(img, ratio, roomW, roomH) {
  return placed(img, { maxW: Math.max(1, roomW), maxH: Math.max(1, roomH), ratio });
}

/** An <img> faded in once it has loaded (`.shown`), as the Claude app's are. */
export function shown(node) {
  const done = () => node.classList.add('shown');
  if (node.complete && node.naturalWidth > 0) done();
  else node.addEventListener('load', done);
  return {
    destroy() {
      node.removeEventListener('load', done);
    },
  };
}

/** "Screenshot removed after 30 days" (the setting's period), in its place. */
export function removedLine(days) {
  return days > 0 ? `Screenshot removed after ${days} days` : 'Screenshot removed';
}

/** "812 × 430" */
export function sizeLine(img) {
  return img?.w && img?.h ? `${img.w} × ${img.h}` : '';
}

/** Whether a failed send was the model not reading the screenshot (snip/vision.rs). */
export function visionRefused(e) {
  const s = String(e ?? '');
  return s.includes("couldn't read the screenshot") || s.includes("can't read images");
}

/**
 * "Copy image": onto the clipboard, out of Win+V history (snip/copy_save.rs).
 * `what`: what it's called ("Screenshot", or "Picture": one Chat found online).
 */
export async function copyImage(id, what = 'Screenshot') {
  try {
    await invoke('chat_image_copy', { id });
    toast({ title: `${what} copied`, variant: 'success' });
  } catch (e) {
    toast({ title: `Couldn't copy the ${what.toLowerCase()}`, description: String(e), variant: 'destructive' });
  }
}

/** "Save image as…": Windows' Save dialog, in the folder used last. */
export async function saveImage(id, what = 'Screenshot') {
  try {
    if (await invoke('chat_image_save', { id })) toast({ title: `${what} saved`, variant: 'success' });
  } catch (e) {
    toast({ title: `Couldn't save the ${what.toLowerCase()}`, description: String(e), variant: 'destructive' });
  }
}

/** A screenshot's right-click menu: open it large (`onview(img)`), copy it, save it. */
export function imageItems(img, { onview = null } = {}) {
  if (!img?.id || img.gone) return [];
  return [
    onview && { label: 'Open', icon: 'open', onselect: () => onview(img) },
    { label: 'Copy image', icon: 'copy', onselect: () => copyImage(img.id) },
    { label: 'Save image as…', icon: 'download', onselect: () => saveImage(img.id) },
  ];
}

/**
 * The picture a paste event carries, if it has one (an image copied in any
 * app). Not when it carries text too: Excel's cells, PowerPoint's or
 * OneNote's text put a picture of themselves on the clipboard beside the
 * words, and WebView2 hands that over as a PNG — the text pastes as ever.
 * (A picture copied in Firefox brings its own address as the text: still
 * the picture.)
 */
export function pastedImage(e) {
  const text = e?.clipboardData?.getData?.('text/plain')?.trim() ?? '';
  if (text && !/^(\S+:\/\/|data:)\S*$/.test(text)) return null;
  const items = [...(e?.clipboardData?.items ?? [])];
  const item = items.find((i) => i.kind === 'file' && i.type.startsWith('image/'));
  return item?.getAsFile() ?? null;
}

/** Keep a pasted picture as a chat screenshot (snip/attach.rs): its bytes as the call's body. */
export async function keepPasted(file) {
  const bytes = new Uint8Array(await file.arrayBuffer());
  return invoke('chat_image_paste', bytes);
}

/**
 * The line before a screenshot's text in a question sent "as text"
 * (snip/ocr.rs `MARK`, `MARK_MANY`): what's after it is folded in the bubble.
 */
export const SCREEN_TEXT_MARKS = [
  '[Text read from the screenshot on this PC, sent instead of the picture]',
  '[Text read from the screenshots on this PC, sent instead of the pictures]',
];

/** A question's own words and the screenshot text that went with them (or null). */
export function splitScreenText(text) {
  const s = String(text ?? '');
  for (const mark of SCREEN_TEXT_MARKS) {
    const at = s.indexOf(`\n\n${mark}\n`);
    if (at >= 0) return { question: s.slice(0, at), screenText: s.slice(at + mark.length + 3) };
  }
  return { question: s, screenText: null };
}
