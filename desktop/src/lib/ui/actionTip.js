// **A small button's tooltip** (16 Nov 2026, as the Claude app's under a
// message): `use:actionTip={{ title, text }}` — after a short hover, or on the
// keyboard's focus, a dark bubble above the button: its name in bold and, when
// there's more to say, a line under it ("Rewind to here" / "Takes this message
// and everything after it out of the conversation"). Below it when there's no
// room above. It floats on the page, so a scrolling thread never clips it, and
// goes on leaving, a press, a scroll or a key. The button keeps its own
// `aria-label`; the bubble is for the eye (`role="tooltip"`, never focused).

const DELAY_MS = 350;
const GAP = 6;
const EDGE = 8;

let bubble = null;
let owner = null;

function bubbleEl() {
  if (!bubble) {
    bubble = document.createElement('div');
    bubble.className = 'yap-action-tip';
    bubble.setAttribute('role', 'tooltip');
    bubble.dataset.testid = 'action-tip';
  }
  return bubble;
}

function hide(node) {
  if (node && owner !== node) return;
  bubble?.remove();
  owner = null;
}

/** `node`: the button. `opts`: `{ title, text? }` (none or no title: no bubble). */
export function actionTip(node, opts = {}) {
  let o = opts;
  let timer = 0;

  function show() {
    const title = String(o?.title ?? '').trim();
    if (!title || !node.isConnected) return;
    const b = bubbleEl();
    const strong = document.createElement('strong');
    strong.textContent = title;
    const parts = [strong];
    const text = String(o?.text ?? '').trim();
    if (text) {
      const line = document.createElement('span');
      line.textContent = text;
      parts.push(line);
    }
    b.replaceChildren(...parts);
    b.classList.toggle('wide', !!text);
    document.body.appendChild(b);
    owner = node;
    const r = node.getBoundingClientRect();
    const bw = b.offsetWidth;
    const bh = b.offsetHeight;
    let left = r.left + r.width / 2 - bw / 2;
    left = Math.max(EDGE, Math.min(left, window.innerWidth - bw - EDGE));
    let top = r.top - bh - GAP;
    if (top < EDGE) top = r.bottom + GAP;
    b.style.left = `${Math.round(left)}px`;
    b.style.top = `${Math.round(top)}px`;
  }

  function soon() {
    clearTimeout(timer);
    timer = setTimeout(show, DELAY_MS);
  }
  function away() {
    clearTimeout(timer);
    hide(node);
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
      // (Open now: its words follow — "Copy" becomes "Copied".)
      if (owner === node) show();
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
