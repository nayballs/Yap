// **An answer's code blocks in colour** (11 Nov 2026; the maintainer, once
// the rows of a Code chat had theirs: "Yes, colour them"): a Svelte action on
// the element an answer's Markdown is rendered into.
//
//   <div class="cbody" use:colourCode>{@html renderMarkdown(text)}</div>
//
// - **Which blocks**: a fenced block that names its language (` ```js `:
//   core/markdown.js puts it on the code as a class, `lang-js`). One with no
//   language, or one the highlighter doesn't know, stays plain: nothing is
//   guessed.
// - **The highlighter** is the rows' (core/codeColours.js: highlight.js in a
//   chunk of its own), loaded the first time a block that names a language
//   shows. Until then the code is plain, as it always was.
// - **Never markup**: the rendered HTML has no colours in it. This reads the
//   block's own text, colours it into tokens (`colourLines`) and puts them
//   back as text nodes in spans (`.syn-k`…, app.css). The text itself is
//   never changed, so copying it gives what was written.
// - **An answer still being written** is rendered again with each piece; the
//   new block is coloured in the same step (a MutationObserver's callback
//   runs before the page is drawn), so the colours don't flicker.
import { colourLines, fenceLanguage, loadColours } from '../core/codeColours.js';

/** The blocks an answer may have in colour: its fenced code that names a language and hasn't been looked at yet. */
const BLOCKS = 'pre.md-code > code[class*="lang-"]:not([data-coloured])';

/** The highlighter, once it has loaded (every answer shares it). */
let colours = null;

/** One block in colour: its text as tokens, each a text node (in a span when it has a colour). `data-coloured`: "true", or "plain" when it couldn't be. */
function colour(code) {
  const lines = colourLines(colours, code.textContent, fenceLanguage(code.className));
  code.dataset.coloured = lines ? 'true' : 'plain';
  if (!lines) return;
  const out = document.createDocumentFragment();
  lines.forEach((line, i) => {
    if (i) out.append('\n');
    for (const t of line) {
      if (!t.kind) {
        out.append(t.text);
        continue;
      }
      const span = document.createElement('span');
      span.className = `syn-${t.kind}`;
      span.textContent = t.text;
      out.append(span);
    }
  });
  code.replaceChildren(out);
}

export function colourCode(node) {
  let here = true;
  const look = () => {
    const blocks = node.querySelectorAll(BLOCKS);
    if (!blocks.length) return;
    if (colours) {
      blocks.forEach(colour);
      return;
    }
    loadColours()
      .then((h) => {
        colours = h;
        if (here) node.querySelectorAll(BLOCKS).forEach(colour);
      })
      .catch(() => {});
  };
  look();
  // Rendered again (an answer still being written, an edit): its new blocks.
  // (This action's own change is seen too, and finds nothing left to do.)
  const seen = new MutationObserver(look);
  seen.observe(node, { childList: true, subtree: true });
  return {
    destroy() {
      here = false;
      seen.disconnect();
    },
  };
}
