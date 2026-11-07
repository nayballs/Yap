// **The full text of a cut-off line, on a hover** (after Claude Code's chat
// list, 2026-11-07): `use:overflowTip` on a row whose `.ctitle` (or, with
// `{ text }`, any text) ends in an ellipsis. After a short hover, or on the
// keyboard's focus, a dark tooltip beside the row shows the whole of it,
// wrapping; never when the line already fits. It floats on the page itself,
// so a scrolling list's edge never clips it, and it goes on leaving, a
// scroll, a click or a key.

const DELAY_MS = 450;
const GAP = 8;

let bubble = null;

function bubbleEl() {
  if (!bubble) {
    bubble = document.createElement('div');
    bubble.className = 'yap-overflow-tip';
    bubble.setAttribute('role', 'tooltip');
    bubble.dataset.testid = 'overflow-tip';
  }
  return bubble;
}

function hide() {
  bubble?.remove();
}

/** Is `el`'s text cut off (an ellipsis showing)? */
function cutOff(el) {
  return !!el && el.scrollWidth > el.clientWidth + 1;
}

/**
 * `node`: the row. `opts.text`: the full text (default: the `.ctitle`
 * inside it); `opts.line`: the element that may be cut off (default the same
 * `.ctitle`, else the row).
 */
export function overflowTip(node, opts = {}) {
  let o = opts;
  let timer = 0;

  const line = () => (o.line ?? node.querySelector('.ctitle') ?? node);
  const text = () => (o.text ?? line()?.textContent ?? '').trim();

  function show() {
    const l = line();
    const t = text();
    if (!t || !cutOff(l) || !node.isConnected) return;
    const b = bubbleEl();
    b.textContent = t;
    document.body.appendChild(b);
    const r = node.getBoundingClientRect();
    const bw = b.offsetWidth;
    const bh = b.offsetHeight;
    // Beside the row, its middle level with the row's; under it when there's
    // no room on the right.
    let left = r.right + GAP;
    let top = r.top + r.height / 2 - bh / 2;
    if (left + bw > window.innerWidth - GAP) {
      left = Math.max(GAP, Math.min(r.left, window.innerWidth - bw - GAP));
      top = r.bottom + GAP;
    }
    top = Math.max(GAP, Math.min(top, window.innerHeight - bh - GAP));
    b.style.left = `${Math.round(left)}px`;
    b.style.top = `${Math.round(top)}px`;
  }

  function soon() {
    clearTimeout(timer);
    timer = setTimeout(show, DELAY_MS);
  }
  function away() {
    clearTimeout(timer);
    hide();
  }
  function focused() {
    if (node.matches(':focus-visible')) soon();
  }

  node.addEventListener('pointerenter', soon);
  node.addEventListener('pointerleave', away);
  node.addEventListener('pointerdown', away);
  node.addEventListener('focus', focused);
  node.addEventListener('blur', away);
  node.addEventListener('keydown', away);
  window.addEventListener('scroll', away, true);

  return {
    update(next) {
      o = next ?? {};
    },
    destroy() {
      away();
      node.removeEventListener('pointerenter', soon);
      node.removeEventListener('pointerleave', away);
      node.removeEventListener('pointerdown', away);
      node.removeEventListener('focus', focused);
      node.removeEventListener('blur', away);
      node.removeEventListener('keydown', away);
      window.removeEventListener('scroll', away, true);
    },
  };
}
