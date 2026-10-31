// A document's sections as documents.rs finds them (`section_range`): pure,
// shared by the panel's state (chatDoc.svelte.js) and its page (docStream.js).
import { isPlaceholder } from '../../core/markdown.js';

/**
 * Section `key`'s lines in `lines`, `[start, end)`: its `##` heading to the
 * next `#`/`##` heading or placeholder (blank lines at its end left out),
 * as documents.rs `section_range` finds it; `null` if there's none.
 */
export function sectionRange(lines, key) {
  const want = String(key).trim().replace(/\s+/g, ' ').toLowerCase();
  const start = lines.findIndex((l) => {
    const m = /^\s*##\s+(.*?)\s*$/.exec(l);
    return !!m && m[1].replace(/#+$/, '').trim().replace(/\s+/g, ' ').toLowerCase() === want;
  });
  if (start < 0) return null;
  let end = start + 1;
  while (end < lines.length && !/^\s*#{1,2}\s+\S/.test(lines[end]) && !isPlaceholder(lines[end])) end++;
  while (end > start + 1 && !lines[end - 1].trim()) end--;
  return [start, end];
}

/** The heading a section's text begins with (`## Gear` → "Gear"), when that line is whole. */
function ownHeading(text) {
  return /^\s*#{1,2}\s+(.+?)\s*#*\s*\n/.exec(String(text ?? ''))?.[1] ?? null;
}

/**
 * Section `key`'s range in `lines`: by the heading its text (`text`, as it
 * streamed) began with — the model's own heading is the section's — else
 * by its key. Pure.
 */
export function findSection(lines, key, text = '') {
  const own = ownHeading(text);
  return (own && sectionRange(lines, own)) || sectionRange(lines, key);
}

/** Section `key` of `content` as saved, its heading and body (`findSection`); `null` if there's none. Pure. */
export function sectionText(content, key, text = '') {
  const lines = String(content || '').split('\n');
  const r = findSection(lines, key, text);
  return r ? lines.slice(r[0], r[1]).join('\n') : null;
}
