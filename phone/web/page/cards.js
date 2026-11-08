// **The cards an agent waits on** (Remote Control, wave 2): a permission
// prompt (Allow once / Deny), a plan (Go ahead / Not yet) and Yap's
// ask-first card before it sends an agent (Start / Don't, Send / Don't for
// a follow-up) — as the PC's own cards say them (AgentApprove.svelte,
// AgentConfirm.svelte; pageConfirm.js's words, shared) — and the
// **Needs-you banner** that shows on every screen while one waits.
//
// **Each answer is a fresh tap** (core/codeWire.js; the PC checks it again,
// link/code/gates.rs): only on a card the PC sent this phone, its buttons
// asleep until it has been on screen `MIN_SHOWN_MS` (from when it was
// actually visible: in view, the page in front), `shownMs` sent with it,
// from a trusted pointer's click — never a script's, never a held key's
// repeat, **never by voice** (nothing spoken maps to an answer here). Then
// "Allowing…" until the PC says how it ended (`approve_done`), and who, when
// another phone or the PC answered first.
//
// A card's node is kept (one a place it shows: the thread, the agent's live
// view) and painted in place, so a thread redrawing while an answer streams
// never takes a button from under a finger.

import { approve, confirm, MIN_SHOWN_MS } from './core/codeWire.js';
import { questionParts, whyOf } from './core/pageConfirm.js';
import { longSaid } from './core/agents.js';
import { renderAnswer } from './markdown.js';
import { h, svg, ICONS } from './dom.js';

/** Plan mode's "may I go ahead?" (integrations/claude_live/approve.rs `EXIT_PLAN`). */
export const EXIT_PLAN = 'ExitPlanMode';
/** An answer the PC hasn't said back on this long is the PC's error's, or lost. */
const SENT_FOR_MS = 8000;

/** `card`'s workspace, as its card names it. Pure. */
export const workspaceOf = (c) => (c?.kind === 'confirm' ? c.agent?.folder : c?.label) || 'your folder';

/** **The banner's words** for the cards waiting. Pure. */
export function bannerWords(list) {
  const n = list?.length || 0;
  if (!n) return '';
  if (n > 1) return `${n} steps need you`;
  const c = list[0];
  if (c.kind === 'confirm') return `Yap asks before sending an agent to ${workspaceOf(c)}`;
  if (c.plan) return `The plan in ${workspaceOf(c)} is ready for you`;
  return `A step in ${workspaceOf(c)} needs you`;
}

/** "on your PC", "from Kitchen tablet", or nothing (this phone). Pure. */
function byWords(by, name) {
  if (by === 'pc') return ' on your PC';
  if (by === 'phone') return ` from ${name || 'another phone'}`;
  return '';
}

/** **What a card says once it's answered** (`outcome`, `by`, `name`). Pure. */
export function doneWords(c) {
  if (c.text) return c.text;
  const by = byWords(c.by, c.name);
  const o = c.outcome;
  if (o === 'timedOut') return c.kind === 'confirm' ? "No answer in time, so Yap didn't send it." : 'No answer in time, so the agent was told no.';
  if (c.kind === 'confirm') {
    if (o === 'allowed') return `${c.agent?.continues ? 'Sent to the agent' : 'Started'}${by}`;
    if (o === 'refused' || o === 'denied') return `Yap didn't send it${by}`;
    return 'Yap stopped asking.';
  }
  if (o === 'allowed') return c.plan ? `Going ahead with the plan${by}` : `Allowed${by}`;
  if (o === 'denied' || o === 'refused') return c.plan ? `Kept as a plan${by}` : `Denied${by}`;
  return 'The agent stopped asking.';
}

/** "Allowing…" while the PC hasn't said yet. Pure. */
export function sendingWords(c) {
  if (c.kind === 'confirm') return c.allow ? (c.agent?.continues ? 'Sending…' : 'Starting…') : 'Saying no…';
  if (c.plan) return c.allow ? 'Going ahead…' : 'Keeping it a plan…';
  return c.allow ? 'Allowing…' : 'Denying…';
}

/** How long it has waited, and when Yap says no by itself. Pure (`nowS`: unix seconds). */
export function timeWords(c, nowS = Date.now() / 1000) {
  const asked = Number(c.askedTs) || 0;
  const ends = Number(c.expiresTs) || 0;
  const parts = [];
  if (asked) {
    const ago = Math.max(0, nowS - asked);
    parts.push(ago < 60 ? 'Asked just now' : `Asked ${Math.round(ago / 60)} min ago`);
  }
  if (ends) {
    const total = asked ? Math.max(0, ends - asked) : Infinity;
    const left = Math.min(total, Math.max(0, ends - nowS));
    const min = Math.max(1, Math.ceil(left / 60));
    parts.push(c.kind === 'confirm' ? `no answer in ${min} min is a no` : `Yap says no in ${min} min`);
  }
  const s = parts.join(' · ');
  return s ? s.charAt(0).toUpperCase() + s.slice(1) : '';
}

/**
 * The cards. `send(msg)` goes to the PC; `onChange()` redraws what shows
 * them (the banner, the title, the dots); `onArrive(card)` is told of a
 * card new to this phone (a vibration, a notification). `now()` is the
 * page's clock (ms).
 */
export function createCards({ send, onChange, onArrive, now = () => performance.now() }) {
  /** key ("approve:7", "confirm:3") → card. */
  const cards = new Map();
  /** Every node shown, for the visibility watch: node → card. */
  const nodes = new Map();
  let repeatAt = 0;

  const io =
    typeof IntersectionObserver === 'function'
      ? new IntersectionObserver(
          (entries) => {
            for (const e of entries) {
              // Half of it on screen (or, a tall one, much of the screen).
              const inView = e.isIntersecting && (e.intersectionRatio >= 0.5 || e.intersectionRect.height >= window.innerHeight * 0.4);
              e.target.__inView = inView;
              if (inView) seen(nodes.get(e.target));
            }
          },
          { threshold: [0, 0.25, 0.5, 0.75, 1] },
        )
      : null;

  /** Its buttons wake `MIN_SHOWN_MS` after it's first on screen, the page in front. */
  function seen(card) {
    if (!card || card.shownAt || document.visibilityState !== 'visible') return;
    card.shownAt = now();
    setTimeout(() => paint(card), MIN_SHOWN_MS + 20);
  }
  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState !== 'visible') return;
    for (const [node, card] of nodes) if (node.isConnected && (node.__inView || !io)) seen(card);
  });
  // A held key's repeats never answer.
  document.addEventListener(
    'keydown',
    (e) => {
      if (e.repeat) repeatAt = performance.now();
    },
    { capture: true },
  );

  const awake = (c) => c.state === 'waiting' && c.shownAt > 0 && now() - c.shownAt >= MIN_SHOWN_MS;

  /** **A card from the PC** (`approve_card`, `confirm_card`, a thread's `cards`): kept; new ones said (`onArrive`). */
  function add(kind, raw, { quiet = false } = {}) {
    let c;
    if (kind === 'approve') {
      const card = raw?.card || raw || {};
      if (typeof card.id !== 'number') return null;
      c = {
        kind,
        id: card.id,
        run: String(card.run || ''),
        conversation: typeof card.conversation === 'number' ? card.conversation : null,
        tool: String(card.tool || ''),
        summary: String(card.summary || ''),
        detail: String(card.detail || ''),
        label: String(card.label || ''),
        askedTs: Number(card.askedTs) || 0,
        expiresTs: Number(raw?.expiresTs) || 0,
        plan: card.tool === EXIT_PLAN,
      };
    } else {
      if (typeof raw?.id !== 'number') return null;
      c = {
        kind,
        id: raw.id,
        run: '',
        conversation: typeof raw.conversation === 'number' ? raw.conversation : null,
        agent: raw.agent && typeof raw.agent === 'object' ? raw.agent : {},
        askedTs: Number(raw.expiresTs) ? Number(raw.expiresTs) - 120 : 0,
        expiresTs: Number(raw.expiresTs) || 0,
        plan: false,
      };
    }
    const key = `${kind}:${c.id}`;
    const was = cards.get(key);
    if (was) {
      // Sent again (a reconnect, the thread): still waiting on the PC.
      Object.assign(was, c);
      if (was.state === 'done') {
        Object.assign(was, { state: 'waiting', outcome: '', by: null, name: '', text: '', error: '' });
      }
      paint(was);
      onChange?.();
      return was;
    }
    const card = { ...c, key, state: 'waiting', allow: null, outcome: '', by: null, name: '', text: '', error: '', shownAt: 0, sentAt: 0, unfold: false, places: new Map() };
    cards.set(key, card);
    if (!quiet) onArrive?.(card);
    onChange?.();
    return card;
  }

  /** **How a card ended** (`approve_done`, `confirm_done`): `{id, outcome, by, name}`. */
  function done(kind, m) {
    const card = cards.get(`${kind}:${m?.id}`);
    if (!card) return;
    card.state = 'done';
    card.outcome = String(m.outcome || 'ended');
    card.by = m.by ?? null;
    card.name = String(m.name || '');
    card.error = '';
    paint(card);
    onChange?.();
  }

  /** The PC's latest dots (`code_status`): a card it no longer lists has ended (missed while away). */
  function prune(status) {
    if (!status) return;
    const waitingIds = {
      approve: new Set((status.approvals || []).map((a) => a.id)),
      confirm: new Set((status.confirms || []).map((a) => a.id)),
    };
    let any = false;
    for (const c of cards.values()) {
      if (c.state === 'done' || waitingIds[c.kind].has(c.id)) continue;
      // An answer on its way: its own end says how it went.
      if (c.state === 'sending' && now() - c.sentAt < SENT_FOR_MS) continue;
      c.state = 'done';
      c.outcome = c.outcome || 'ended';
      paint(c);
      any = true;
    }
    if (any) onChange?.();
  }

  /** **The PC said no to an answer** (`error`): back to waiting with why, or over ("Already allowed from …"). True when it was a card's. */
  function onError(message) {
    const c = [...cards.values()].find((x) => x.state === 'sending' && now() - x.sentAt < SENT_FOR_MS);
    if (!c) return false;
    if (/^Already |isn't waiting|any more/.test(message)) {
      c.state = 'done';
      c.text = message.replace(/\.$/, '');
    } else {
      c.state = 'waiting';
      c.error = message;
    }
    paint(c);
    onChange?.();
    return true;
  }

  /** **A tap on one of its buttons**: the PC asked, "Allowing…" until it answers. */
  function answer(card, yes, e) {
    if (!e?.isTrusted || performance.now() - repeatAt < 600) return;
    if (!awake(card)) return;
    const shownMs = now() - card.shownAt;
    const msg = card.kind === 'approve' ? approve(card.id, yes, shownMs) : confirm(card.id, yes, shownMs);
    if (!send(msg)) {
      card.error = "Not connected to your PC: try again once it's back.";
      paint(card);
      return;
    }
    card.state = 'sending';
    card.allow = !!yes;
    card.sentAt = now();
    card.error = '';
    paint(card);
    onChange?.();
  }

  // ---- a card's node ----

  function build(card) {
    const p = {};
    const node = h('div', {
      class: `ccard kind-${card.kind}${card.plan ? ' plan' : ''}`,
      role: 'group',
      testid: 'code-card',
      data: { kind: card.plan ? 'plan' : card.kind, id: card.id },
    });
    const icon = svg(card.kind === 'confirm' ? ICONS.folder : card.plan ? ICONS.plan : ICONS.shield, 'cicon');
    const body = h('div', { class: 'cbody' });
    if (card.kind === 'approve' && !card.plan) {
      node.setAttribute('aria-label', 'An agent asks to take a step');
      body.append(
        h('p', { class: 'cq' }, 'The agent in ', h('strong', { class: 'cws', text: workspaceOf(card) }), ' asks to:'),
        h('p', { class: 'csummary', testid: 'card-summary', text: card.summary }),
      );
      if (card.detail.trim() && card.detail.trim() !== card.summary.trim()) {
        body.append(h('pre', { class: 'cdetail', tabindex: '0', 'aria-label': 'What exactly it would do', testid: 'card-detail', text: card.detail }));
      }
    } else if (card.plan) {
      node.setAttribute('aria-label', "The agent's plan is ready");
      body.append(h('p', { class: 'cq' }, h('strong', { text: 'The plan is ready' }), ' · ', h('span', { class: 'cws', text: workspaceOf(card) })));
      if (card.detail.trim()) {
        const plan = h('div', { class: 'cplan', testid: 'card-plan' }, renderAnswer(card.detail));
        body.append(plan);
        if (longSaid(card.detail)) {
          plan.classList.add('folded');
          const more = h('button', {
            type: 'button',
            class: 'cfold',
            'aria-expanded': 'false',
            text: 'Show more',
            on: {
              click: () => {
                card.unfold = !card.unfold;
                plan.classList.toggle('folded', !card.unfold);
                more.textContent = card.unfold ? 'Show less' : 'Show more';
                more.setAttribute('aria-expanded', String(card.unfold));
              },
            },
          });
          body.append(more);
        }
      }
    } else {
      node.setAttribute('aria-label', 'Yap asks before sending an agent');
      const a = card.agent || {};
      const q = questionParts({ act: 'agent', label: a.folder || 'your folder', agent: a });
      body.append(h('p', { class: 'cq' }, `${q.lead} `, h('strong', { class: 'cws', text: `‘${q.quoted}’` }), q.then));
      if (a.path) body.append(h('p', { class: 'cpath', text: a.path }));
      if (a.brief) {
        body.append(
          h('p', { class: 'clabel', text: 'What Claude Code would be asked' }),
          h('div', { class: 'cdetail brief', tabindex: '0', role: 'region', 'aria-label': "The agent's brief", testid: 'card-brief', text: a.brief }),
        );
      }
    }
    p.time = h('p', { class: 'ctime', testid: 'card-time' });
    p.why = h('p', {
      class: 'cwhy',
      text:
        card.kind === 'confirm'
          ? `${whyOf({ act: 'agent', agent: card.agent })} Only you can say yes.`
          : card.plan
            ? "Go ahead and it carries the plan out at this chat's Mode. Not yet keeps it a plan, with nothing changed."
            : `${card.tool ? `${card.tool} · ` : ''}Only you can allow it, and only for this one step.`,
    });
    p.error = h('p', { class: 'cerror', role: 'alert', testid: 'card-error', hidden: true });
    const yesWord = card.kind === 'confirm' ? (card.agent?.continues ? 'Send' : 'Start') : card.plan ? 'Go ahead' : 'Allow once';
    const noWord = card.kind === 'confirm' ? "Don't" : card.plan ? 'Not yet' : 'Deny';
    p.no = h('button', { type: 'button', class: 'cardbtn no', testid: 'card-no', text: noWord, on: { click: (e) => answer(card, false, e) } });
    p.yes = h('button', { type: 'button', class: 'cardbtn yes', testid: 'card-yes', text: yesWord, on: { click: (e) => answer(card, true, e) } });
    p.btns = h('div', { class: 'cbtns' }, p.no, p.yes);
    p.status = h('p', { class: 'cstatus', role: 'status', testid: 'card-status', hidden: true });
    body.append(p.time, p.why, p.error, p.btns, p.status);
    node.append(icon, body);
    node.__parts = p;
    return node;
  }

  function paint(card) {
    for (const node of card.places.values()) paintNode(card, node);
  }

  function paintNode(card, node) {
    const p = node.__parts;
    node.dataset.state = card.state;
    const asleep = !awake(card);
    p.no.disabled = asleep;
    p.yes.disabled = asleep;
    p.btns.hidden = card.state !== 'waiting';
    p.time.hidden = card.state !== 'waiting';
    p.why.hidden = card.state === 'done';
    if (card.state === 'waiting') p.time.textContent = timeWords(card);
    p.error.hidden = !card.error;
    p.error.textContent = card.error;
    p.status.hidden = card.state === 'waiting';
    p.status.textContent = card.state === 'sending' ? sendingWords(card) : card.state === 'done' ? doneWords(card) : '';
    p.status.dataset.outcome = card.state === 'done' ? card.outcome || 'over' : '';
  }

  /**
   * **The cards for one place** into `root` (`place`: its name; `which(card)`
   * says which belong there), oldest first; their nodes kept and painted.
   */
  function mount(root, place, which) {
    const want = [...cards.values()].filter(which).sort((a, b) => a.askedTs - b.askedTs || a.id - b.id);
    const keep = new Set();
    for (const card of want) {
      let node = card.places.get(place);
      if (!node) {
        node = build(card);
        card.places.set(place, node);
        nodes.set(node, card);
        io?.observe(node);
      }
      paintNode(card, node);
      keep.add(node);
    }
    for (const node of [...root.children]) {
      if (!keep.has(node)) {
        const card = nodes.get(node);
        if (card) card.places.delete(place);
        nodes.delete(node);
        io?.unobserve(node);
        node.remove();
      }
    }
    // In order, without moving a node that's already where it goes.
    let at = root.firstChild;
    for (const card of want) {
      const node = card.places.get(place);
      if (node === at) at = at.nextSibling;
      else root.insertBefore(node, at);
    }
    root.hidden = want.length === 0;
    // Not watched (no IntersectionObserver): shown once the page is.
    if (!io) for (const card of want) seen(card);
  }

  // The time words, kept current while a card waits.
  setInterval(() => {
    for (const c of cards.values()) if (c.state === 'waiting') paint(c);
  }, 20_000);

  return {
    add,
    done,
    prune,
    onError,
    mount,
    /** The cards still waiting on the person (and those whose answer is on its way). */
    waiting: () => [...cards.values()].filter((c) => c.state === 'waiting' || c.state === 'sending'),
    /** Those waiting in conversation `conv`, or for agent `run`. */
    of: (pred) => [...cards.values()].filter(pred),
    get: (kind, id) => cards.get(`${kind}:${id}`) || null,
    /** A card's node in a place, to bring into view. */
    nodeOf: (card, place) => card?.places.get(place) || null,
    /** Remote Control went off: no cards. */
    clear() {
      for (const node of nodes.keys()) {
        io?.unobserve(node);
        node.remove();
      }
      nodes.clear();
      cards.clear();
      onChange?.();
    },
    /** The tests' view: each card's state. */
    snapshot: () => [...cards.values()].map((c) => ({ kind: c.kind, id: c.id, state: c.state, outcome: c.outcome, by: c.by, shown: c.shownAt > 0 })),
  };
}
