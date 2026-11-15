// **Copy on an answer's code blocks** (15 Nov 2026, the Claude app's Copy on
// every block): a Svelte action beside `colourCode` on the element an
// answer's Markdown is rendered into.
//
//   <div class="cbody" use:colourCode use:codeCopy>{@html renderMarkdown(text)}</div>
//
// Every `pre.md-code` (core/markdown.js) gets a small button in its top-right
// corner that copies the block's code exactly as written: the code element's
// own text, which colouring never changes (ui/colourCode.js). The button has
// no text of its own, so selecting and copying the block by hand gives the
// code alone too. An answer still being written is rendered again with each
// piece: its blocks get their buttons in the same step (a MutationObserver's
// callback runs before the page is drawn). The look: app.css `.md-codecopy`.
import { copyText } from '../core/itemMenus.js';

const BLOCKS = 'pre.md-code:not([data-copy])';

const ICON =
  '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="8" y="8" width="12" height="12" rx="2"/><path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2"/></svg>';

/** The code a block's button copies: its `code` element's text, the trailing newline aside. */
export function blockCode(pre) {
  return (pre.querySelector('code')?.textContent ?? '').replace(/\n$/, '');
}

function addButton(pre) {
  pre.dataset.copy = '';
  const b = document.createElement('button');
  b.type = 'button';
  b.className = 'md-codecopy';
  b.title = 'Copy code';
  b.setAttribute('aria-label', 'Copy code');
  b.dataset.testid = 'md-code-copy';
  b.innerHTML = ICON;
  pre.append(b);
}

export function codeCopy(node) {
  const look = () => node.querySelectorAll(BLOCKS).forEach(addButton);
  look();
  // Rendered again (an answer still being written): its new blocks. (Its own
  // buttons are seen too, and find nothing left to do.)
  const seen = new MutationObserver(look);
  seen.observe(node, { childList: true, subtree: true });
  // One listener for every block's button (they come and go with the HTML).
  const click = (e) => {
    const b = e.target instanceof Element ? e.target.closest('button.md-codecopy') : null;
    if (!b || !node.contains(b)) return;
    e.preventDefault();
    e.stopPropagation();
    const pre = b.closest('pre.md-code');
    if (pre) copyText(blockCode(pre), 'Code copied');
  };
  node.addEventListener('click', click);
  return {
    destroy() {
      seen.disconnect();
      node.removeEventListener('click', click);
    },
  };
}
