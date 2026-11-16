// **Files attached to a question** (chat/attachments.rs, 15 Nov 2026): what the box and the thread say about them.
// Pure, so the e2e suite checks them without the app (chat-files.spec.js).
import { badgeOf, fileSize } from './fileChip.js';

/** Endings dropped on Chat that go in as pictures (snip/attach.rs reads them), not as files. */
const PICTURES = new Set(['png', 'jpg', 'jpeg', 'gif', 'bmp', 'webp', 'tif', 'tiff', 'heic', 'heif']);

/** A path's ending, lower case ('' for none). */
export function endingOf(path) {
  const name = String(path ?? '').split(/[\\/]/).pop() || '';
  const dot = name.lastIndexOf('.');
  return dot > 0 ? name.slice(dot + 1).toLowerCase() : '';
}

/** Whether a dropped path is a picture (Chat's screenshots take it). */
export const isPicturePath = (path) => PICTURES.has(endingOf(path));

/** The chip's badge and size (every file chip's: fileChip.js): "PDF", "MD", "DOCX"; "572B", "2.1 KB". */
export { badgeOf, fileSize };

/** A PDF's pages read with OCR (scans: Windows' text recognition, on this PC), and those left unread: "2 pages read
 *  with OCR", "20 pages read with OCR, 28 not read"; '' for none. */
export function ocrLine(file) {
  const n = Number(file?.ocrPages) || 0;
  const left = Number(file?.unreadPages) || 0;
  if (!n && !left) return '';
  const read = n ? `${n} page${n === 1 ? '' : 's'} read with OCR` : '';
  const unread = left ? `${left} page${left === 1 ? '' : 's'} with no text not read` : '';
  return n && left ? `${read}, ${left} not read` : read || unread;
}

/** Tokens said roughly ("about 1.2k tokens"). */
export function tokensLine(n) {
  const t = Math.max(0, Number(n) || 0);
  const said = t >= 1_000_000 ? `${(t / 1_000_000).toFixed(1)}M` : t >= 10_000 ? `${Math.round(t / 1000)}k` : t >= 1000 ? `${(t / 1000).toFixed(1)}k` : String(t);
  return `about ${said} token${t === 1 ? '' : 's'}`;
}

/** The chip's tooltip: name, size, pages, its text's tokens; or that it was removed. */
export function fileTitle(file) {
  if (file?.gone) return `${file.name}: removed (Settings → Privacy & data → Keep chat files)`;
  const parts = [file?.name, fileSize(file?.bytes)];
  if (file?.pages) parts.push(`${file.pages} page${file.pages === 1 ? '' : 's'}`);
  if (ocrLine(file)) parts.push(ocrLine(file));
  if (file?.tokens) parts.push(`${tokensLine(file.tokens)} of text`);
  return parts.filter(Boolean).join(' · ');
}

/** What the files add to a question's size (Rust's `attachments::tokens_of`). */
export function filesTokens(files) {
  const list = Array.isArray(files) ? files : [];
  return list.length ? list.reduce((n, f) => n + (Number(f?.tokens) || 0) + 24, 0) + 8 : 0;
}

/** The question asked when only files are sent (an empty box). */
export const filesPrompt = (files) => (files?.length > 1 ? "What's in these files?" : "What's in this file?");

/** The line for files that weren't added: each with why, as Rust said it. */
export function skippedLine(skipped) {
  const list = (Array.isArray(skipped) ? skipped : []).filter((s) => s?.why);
  if (!list.length) return '';
  return list.map((s) => s.why).join(' ');
}
