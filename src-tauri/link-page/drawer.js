// The drawer of chats, as the Claude app has it: it slides in from the left
// over a scrim (☰ in the header) — Yap's wordmark in its display serif, a
// search box that filters the titles, **Recents** (Chat's conversations on
// the PC: not archived, not a meeting's Ask thread, newest first — `chats`,
// link/chats.rs) with the one this phone is in marked, and at the foot the
// PC's name and "+ New chat". Built as DOM nodes (textContent), never HTML.

const NS = 'http://www.w3.org/2000/svg';
const BUBBLE = 'M5 6.5A2.5 2.5 0 0 1 7.5 4h9A2.5 2.5 0 0 1 19 6.5v7a2.5 2.5 0 0 1-2.5 2.5H11l-4 3.5V16h0a2 2 0 0 1-2-2z';

function bubble() {
  const s = document.createElementNS(NS, 'svg');
  s.setAttribute('viewBox', '0 0 24 24');
  s.setAttribute('aria-hidden', 'true');
  s.setAttribute('focusable', 'false');
  const p = document.createElementNS(NS, 'path');
  p.setAttribute('d', BUBBLE);
  s.appendChild(p);
  return s;
}

/** A title's words to match: lower case, without accents. Pure. */
export function words(s) {
  return String(s || '')
    .normalize('NFD')
    .replace(/[̀-ͯ]/g, '')
    .toLowerCase();
}

/** The items whose title has every word of `query`. Pure. */
export function filter(items, query) {
  const want = words(query).split(/\s+/).filter(Boolean);
  if (!want.length) return items;
  return items.filter((i) => {
    const t = words(i.title);
    return want.every((w) => t.includes(w));
  });
}

/**
 * The drawer. `send(msg)` goes to the PC (`chats` when it opens);
 * `onOpen(item)` / `onNew()` are the page's; `current()` is the conversation
 * this phone is in (an id, or none).
 */
export function createDrawer({ dialog, list, empty, search, pc, newChat, closeBtn, send, onOpen, onNew, current }) {
  let items = [];
  let loaded = false;

  function build() {
    const shown = filter(items, search.value);
    const now = current();
    const frag = document.createDocumentFragment();
    for (const item of shown) {
      const li = document.createElement('li');
      const b = document.createElement('button');
      b.type = 'button';
      b.className = 'recent';
      b.dataset.testid = 'recent';
      b.dataset.id = String(item.id);
      if (item.id === now) b.setAttribute('aria-current', 'true');
      b.appendChild(bubble());
      const t = document.createElement('span');
      t.className = 'recent-title';
      t.textContent = item.title;
      b.appendChild(t);
      b.addEventListener('click', () => {
        close();
        if (item.id !== current()) onOpen(item);
      });
      li.appendChild(b);
      frag.appendChild(li);
    }
    list.replaceChildren(frag);
    empty.hidden = shown.length > 0 || !loaded;
    empty.textContent = items.length ? 'No chats match that.' : 'No chats yet. Ask something to start one.';
  }

  function open() {
    search.value = '';
    send({ type: 'chats' });
    build();
    if (typeof dialog.showModal === 'function') {
      if (!dialog.open) dialog.showModal();
    } else dialog.setAttribute('open', '');
    // The one this phone is in, in view.
    const cur = list.querySelector('[aria-current="true"]');
    if (cur) cur.scrollIntoView({ block: 'nearest' });
  }

  function close() {
    if (typeof dialog.close === 'function' && dialog.open) dialog.close();
    else dialog.removeAttribute('open');
  }

  /** The PC's list (`chats`). */
  function update(next) {
    items = Array.isArray(next)
      ? next.filter((i) => i && typeof i.id === 'number').map((i) => ({ id: i.id, title: String(i.title || 'New chat'), ts: Number(i.ts) || 0 }))
      : [];
    loaded = true;
    if (dialog.open) build();
  }

  /** A conversation's title changed (`title`, `chat`): kept, and first when it's new. */
  function titled(id, title) {
    const i = items.findIndex((x) => x.id === id);
    if (i >= 0) items[i] = { ...items[i], title };
    else items.unshift({ id, title, ts: Math.floor(Date.now() / 1000) });
    if (dialog.open) build();
  }

  function setPc(name) {
    pc.textContent = name;
  }

  search.addEventListener('input', build);
  newChat.addEventListener('click', () => {
    close();
    onNew();
  });
  closeBtn.addEventListener('click', close);
  dialog.addEventListener('click', (e) => {
    // A tap on the scrim (outside the drawer itself) closes it.
    if (e.target !== dialog) return;
    const r = dialog.getBoundingClientRect();
    if (e.clientX < r.left || e.clientX > r.right || e.clientY < r.top || e.clientY > r.bottom) close();
  });

  return {
    open,
    close,
    update,
    titled,
    setPc,
    get isOpen() {
      return !!dialog.open;
    },
    get items() {
      return items;
    },
  };
}
