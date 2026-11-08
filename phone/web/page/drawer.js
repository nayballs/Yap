// The drawer of chats, as the Claude app has it: it slides in from the left
// over a scrim (☰ in the header) — Yap's wordmark in its display serif, a
// search box that filters the titles, **Recents** (Chat's conversations on
// the PC: not archived, not a meeting's Ask thread, newest first — `chats`,
// link/chats.rs) with the one this phone is in marked, and at the foot the
// PC's name and "+ New chat". Built as DOM nodes (textContent), never HTML.
//
// With Remote Control on (Yap Code from the phone, wave 2), a **Chat | Code**
// switch at its top, as the desktop's ChatSideSwitch: Chat lists the plain
// conversations (each with its status dot), Code the Code side (code.js:
// the agents working, then each workspace's Code chats, + New code chat).
// The side not shown carries its strongest dot.

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
export function createDrawer({ dialog, list, empty, search, pc, newChat, newChatLabel, closeBtn, send, onOpen, onNew, current, sides, code }) {
  let items = [];
  let loaded = false;
  // 'chat' or 'code' (the Code side only while Remote Control is on).
  let side = 'chat';
  const codeOn = () => !!code?.on();

  function build() {
    const coding = codeOn();
    if (!coding) side = 'chat';
    if (sides) {
      sides.root.hidden = !coding;
      for (const [name, b] of [
        ['chat', sides.chat],
        ['code', sides.code],
      ]) {
        b.setAttribute('aria-selected', String(side === name));
      }
      sides.chatSide.hidden = side !== 'chat';
      sides.codeSide.hidden = side !== 'code';
      // The other side's strongest dot, on its segment.
      for (const [name, at] of [
        ['chat', sides.chatDot],
        ['code', sides.codeDot],
      ]) {
        const s = coding && side !== name ? code.sideDot(name, plain().map((i) => i.id)) : null;
        at.replaceChildren(...(s ? [code.dot(s, { code: name === 'code', place: 'badge', testid: `side-${name}-dot` })] : []));
      }
    }
    if (newChatLabel) newChatLabel.textContent = side === 'code' ? 'New code chat' : 'New chat';
    if (side === 'code') {
      code.render(sides.codeSide, close);
      return;
    }
    const shown = filter(plain(), search.value);
    const now = current();
    const statuses = coding ? code.statuses() : {};
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
      if (coding) b.appendChild(code.dot(statuses[String(item.id)] ?? null, { testid: 'recent-dot' }));
      b.addEventListener('click', () => {
        close();
        if (item.id !== current()) onOpen(item);
      });
      li.appendChild(b);
      frag.appendChild(li);
    }
    list.replaceChildren(frag);
    empty.hidden = shown.length > 0 || !loaded;
    empty.textContent = plain().length ? 'No chats match that.' : 'No chats yet. Ask something to start one.';
  }

  /** The Chat side's conversations: with Remote Control on, the plain ones (a Code chat is on the Code side). */
  function plain() {
    return codeOn() ? items.filter((i) => !i.code) : items;
  }

  /** **Open the drawer**, on `at`'s side (the side of the chat this phone is in, else the one shown last). */
  function open(at = null) {
    search.value = '';
    if (at === 'code' || at === 'chat') side = at;
    send({ type: 'chats' });
    if (side === 'code' || codeOn()) code?.refresh();
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
      ? next
          .filter((i) => i && typeof i.id === 'number')
          .map((i) => ({ id: i.id, title: String(i.title || 'New chat'), ts: Number(i.ts) || 0, ...(typeof i.code === 'string' && i.code ? { code: i.code } : {}) }))
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
    if (side === 'code') {
      // A sheet of workspaces over the drawer, then the chat.
      code.newChat(close);
      return;
    }
    close();
    onNew();
  });
  for (const b of sides ? [sides.chat, sides.code] : []) {
    b.addEventListener('click', () => {
      side = b.dataset.side === 'code' && codeOn() ? 'code' : 'chat';
      if (side === 'code') code.refresh();
      build();
    });
  }
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
    /** Something it shows changed (the dots, the Code side): drawn again while it's open. */
    refresh() {
      if (dialog.open) build();
    },
    get side() {
      return side;
    },
    get isOpen() {
      return !!dialog.open;
    },
    get items() {
      return items;
    },
  };
}
