// **@-mention a file in a Code chat** (agents/mention.rs, 15 Nov 2026, as Claude Code's own `@`): what's being typed
// after an @, the mention written into the message, and a sent message's mentions found again for its pills. Pure, so
// the e2e suite checks them without the app (chat-mentions.spec.js).

/**
 * The mention being typed at the caret: an `@` at the start or after a space, then no space (or, after `@"`, no
 * quote). `{ start, query }` (`start`: the @'s index), or null.
 */
export function mentionAt(text, caret) {
  const before = String(text ?? '').slice(0, caret ?? String(text ?? '').length);
  const quoted = before.match(/(^|\s)@"([^"]*)$/);
  if (quoted) return { start: before.length - quoted[2].length - 2, query: quoted[2] };
  const m = before.match(/(^|\s)@([^\s@"]*)$/);
  if (!m) return null;
  return { start: before.length - m[2].length - 1, query: m[2] };
}

/** How a mention of `path` is written: `@path`, or `@"path"` with a space in it (Claude Code's way). */
export const written = (path) => (/\s/.test(path) ? `@"${path}"` : `@${path}`);

/**
 * The box's text with the mention typed at `at` (from mentionAt) replaced by the file `path`, and a space after it:
 * `{ text, caret }`.
 */
export function withMention(text, at, path, caret) {
  const t = String(text ?? '');
  const end = caret ?? t.length;
  const tail = t.slice(end);
  const put = `${written(path)}${tail.startsWith(' ') ? '' : ' '}`;
  const out = t.slice(0, at.start) + put + tail;
  // (The caret after the space that follows it, its own or the one already there.)
  return { text: out, caret: at.start + put.length + (tail.startsWith(' ') ? 1 : 0) };
}

/** A path-like mention: a name with an ending, or a folder (a `/` in it). Never an email (`a@b.c`) or `@Sam`; a
 * sentence's full stop after it isn't part of it. */
const MENTION = /(^|\s)@(?:"([^"\n]+)"|((?:[\w.\-]+\/)*[\w\-]+\.[\w]+|(?:[\w.\-]+\/)+(?:[\w.\-]*[\w\-])?))(?=$|[\s,;:!?)\]]|\.(?:\s|$))/g;

/** A sent message's text in parts: plain text, and `{ path }` for each file mentioned (its pill). */
export function mentionParts(text) {
  const s = String(text ?? '');
  const out = [];
  let last = 0;
  for (const m of s.matchAll(MENTION)) {
    const at = m.index + m[1].length;
    if (at > last) out.push({ text: s.slice(last, at) });
    out.push({ path: m[2] ?? m[3] });
    last = m.index + m[0].length;
  }
  if (last < s.length) out.push({ text: s.slice(last) });
  return out;
}

/** The files a message mentions, in order, once each. */
export const mentionedPaths = (text) => [...new Set(mentionParts(text).filter((p) => p.path).map((p) => p.path))];
