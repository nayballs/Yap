// The words typed in a search box (Notes, Chat's sidebar), found again
// where a result opens it: marked in a page's text with the CSS Custom
// Highlight API (`::highlight(yap-found)` in app.css — no change to the
// page's elements, so a note's rendered summary and a chat's answers stay
// as they are) or selected in a text box (a note's My thoughts). A word
// matches where it starts a word, case aside, as much of it as was typed —
// the search's own rule (src-tauri/src/search/library.rs `holds`).

/** The words as the search reads them: lower case, split at anything but
 *  letters and digits, once each. */
export function typedWords(query) {
  const out = [];
  for (const w of String(query || '')
    .toLowerCase()
    .split(/[^\p{L}\p{N}]+/u)) {
    if (w && !out.includes(w)) out.push(w);
  }
  return out;
}

const isWordChar = (c) => /[\p{L}\p{N}]/u.test(c);

/** Where each of `words` starts a word in `text`: `[start, end]` pairs
 *  (string offsets), in order. */
export function wordSpans(text, words) {
  const lower = String(text || '').toLowerCase();
  const spans = [];
  if (lower.length !== String(text || '').length) return spans; // a case change moved letters: leave it
  for (const w of words) {
    if (!w) continue;
    let i = lower.indexOf(w);
    while (i >= 0) {
      if (i === 0 || !isWordChar(lower[i - 1])) spans.push([i, i + w.length]);
      i = lower.indexOf(w, i + 1);
    }
  }
  return spans.sort((a, b) => a[0] - b[0]);
}

let clearTimer = null;

/** Mark `words` in `root`'s text and scroll the first into view (centred).
 *  The marks go after `ms`, or at the next call. Returns how many. */
export function markWords(root, words, { ms = 4000, scroll = true } = {}) {
  clearMarks();
  if (!root || !words?.length || typeof CSS === 'undefined' || !CSS.highlights || typeof Highlight === 'undefined') return 0;
  const ranges = [];
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    for (const [s, e] of wordSpans(node.nodeValue, words)) {
      const r = new Range();
      r.setStart(node, s);
      r.setEnd(node, e);
      ranges.push(r);
    }
  }
  if (!ranges.length) return 0;
  CSS.highlights.set('yap-found', new Highlight(...ranges));
  if (scroll) ranges[0].startContainer.parentElement?.scrollIntoView({ block: 'center' });
  clearTimer = setTimeout(clearMarks, ms);
  return ranges.length;
}

export function clearMarks() {
  clearTimeout(clearTimer);
  clearTimer = null;
  if (typeof CSS !== 'undefined' && CSS.highlights) CSS.highlights.delete('yap-found');
}

/** In a text box: select the first of `words` on line `line` (0-based; any
 *  line when it isn't there), and scroll so it shows. Returns whether one
 *  was found. */
export function selectWords(el, words, line = null) {
  if (!el || !words?.length) return false;
  const text = el.value || '';
  const lines = text.split('\n');
  let offset = 0;
  let found = null;
  if (line != null && line < lines.length) {
    for (let i = 0; i < line; i++) offset += lines[i].length + 1;
    const span = wordSpans(lines[line], words)[0];
    if (span) found = [offset + span[0], offset + span[1]];
  }
  if (!found) {
    const span = wordSpans(text, words)[0];
    if (span) found = span;
  }
  if (!found) return false;
  // Roughly where it is (lines wrap), then the caret's own scroll.
  const before = text.slice(0, found[0]).split('\n').length - 1;
  el.scrollTop = Math.max(0, (before / Math.max(lines.length, 1)) * el.scrollHeight - el.clientHeight / 3);
  el.focus({ preventScroll: true });
  el.setSelectionRange(found[0], found[1]);
  return true;
}
