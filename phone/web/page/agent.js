// **An agent's live view** (Remote Control, wave 2): what the person's own
// Claude Code is doing on the PC, as its tab beside a Code chat shows it
// (AgentTab.svelte, AgentTurn.svelte) — read with Yap Code's own rules
// (core/agents.js). It takes the thread's place; ✕ goes back to the chat
// and never stops the agent; the composer under it is "Tell the agent…".
//
// - Its status, workspace, Mode, model and how long it has worked; what it
//   costs and how it's paid ("Your Claude plan · ≈ $0.04 on the API"); a
//   problem with Claude Code in plain words (its sign-in is on the PC).
// - The line it's on now, Claude Code's to-do list, its turns (the newest
//   open, the earlier folded, one queued a line), each with its timeline —
//   steps and what it said between them, long words folded to four lines —
//   following the live end until you scroll up (↓ to follow again); the
//   files it changed with +N −M, one diff opened at a time on request
//   (`agent_diff`, diff.js); the commands it ran (three, then Show N more);
//   what it wasn't allowed to do; its final words.
// - Its cards (Allow once / Deny, a plan's Go ahead): cards.js, in place.
// - Stop while it works, Continue once it stopped, Go ahead with a plan,
//   Undo this turn / Undo all after the phone's own confirm (`sure`).
// - Older steps on request (`agent_log`), and opened again when the PC
//   says the phone fell behind (`agent_resync`) or a number's missing.
//
// The PC sends a snapshot (`agent_snapshot`: the agent and its last 200
// events, diff lines left out) and then its live events (`agent_event`,
// numbered from 1); this view merges them by their line in the agent's log.

import { LIMITS } from './core/codeWire.js';
import {
  isLive,
  statusWord,
  statusTone,
  costLine,
  money,
  elapsed,
  workedFor,
  turnsOf,
  turnStatusWord,
  timelineOf,
  latestStep,
  todosOf,
  todoCount,
  diffsOf,
  commandsOf,
  problemWords,
  modeLabel,
  modelLabel,
  canUndo,
  canUndoTurn,
  planWaits,
  longSaid,
  NO_BACKUPS,
} from './core/agents.js';
import { renderAnswer } from './markdown.js';
import { fileFacts, counts, renderDiff } from './diff.js';
import { h, svg, fill, ICONS } from './dom.js';

/** Commands shown before "Show N more" (AgentTurn.svelte's). */
const FEW_COMMANDS = 3;

/** An event as this view keeps it: a diff's lines left out by the PC read as none (its counts kept). Pure. */
export function normEvent(e) {
  if (e && e.diff && !Array.isArray(e.diff.lines)) return { ...e, diff: { ...e.diff, lines: [] } };
  return e;
}

/**
 * The view. `els`: its root, head (title, status, ✕), body (scrolls),
 * foot (its buttons), follow (↓), the undo sheet's parts; `send(msg)` to
 * the PC; `cards` (cards.js); `onClose()` back to the chat; `onChange()`
 * redraws the page (the composer's words); `pcName()`.
 */
export function createAgentView({ els, send, cards, onClose, onChange, pcName }) {
  const V = {
    id: '',
    agent: null,
    /** line → event; events the PC didn't number, in order. */
    log: new Map(),
    loose: [],
    more: false,
    seq: 0,
    loading: false,
    folded: {},
    asked: null,
    file: null,
    allCommands: {},
    unfolded: new Set(),
    pinned: true,
    busy: '',
    note: '',
    error: '',
    undo: null,
    wentAhead: {},
    holding: false,
    dirty: false,
    refreshTimer: 0,
    tick: 0,
  };
  // The cards' place, kept across redraws (their buttons stay under a finger).
  const cardsRoot = h('div', { class: 'avcards', testid: 'agent-cards', hidden: true });
  const info = h('div', { class: 'avinfo' });
  const turnsRoot = h('div', { class: 'avturns' });
  // (At the live end, where it waits: in view while the view follows it.)
  els.body.replaceChildren(info, turnsRoot, cardsRoot);

  const isOpen = () => !!V.id && !els.root.hidden;
  const shown = () => V.agent;
  const events = () => [...[...V.log.entries()].sort((a, b) => a[0] - b[0]).map(([, e]) => e), ...V.loose];

  // ---- open, close, the PC's messages ----

  /** **Open agent `brief`** (a pill, a report's Open, the running list), at its turn `turn` when asked. */
  function open(brief, { turn = null } = {}) {
    if (!brief?.id) return;
    const again = V.id === brief.id;
    if (!again) {
      Object.assign(V, { agent: { ...brief, files: [], commands: [], denials: [] }, log: new Map(), loose: [], more: false, seq: 0, folded: {}, file: null, allCommands: {}, unfolded: new Set(), note: '', error: '', busy: '', undo: null, pinned: true });
    }
    V.id = brief.id;
    V.asked = turn;
    if (turn != null) V.folded = { [turn]: false };
    V.loading = true;
    els.root.hidden = false;
    send({ type: 'agent_open', id: brief.id });
    clearInterval(V.tick);
    // How long it has worked, kept current (its words only: nothing redrawn under a finger or a scroll).
    V.tick = setInterval(() => isOpen() && isLive(V.agent?.status) && !V.holding && drawTimes(), 1000);
    render();
    onChange?.();
  }

  /** ✕: back to the chat; the agent carries on. */
  function close() {
    if (!V.id) return;
    send({ type: 'agent_close' });
    V.id = '';
    clearInterval(V.tick);
    clearTimeout(V.refreshTimer);
    els.root.hidden = true;
    closeUndo();
    onClose?.();
    onChange?.();
  }

  /** `agent_snapshot`: the agent as it is and its latest events. */
  function onSnapshot(m) {
    const a = m.agent;
    if (!a || a.id !== V.id) return;
    V.agent = { ...V.agent, ...a };
    for (const x of m.events || []) if (Number.isInteger(x?.line)) V.log.set(x.line, normEvent(x.event));
    V.loose = [];
    // (An older page asked for already, kept: the snapshot's "more" is about what's before it.)
    if (V.log.size <= (m.events || []).length) V.more = !!m.more;
    V.seq = Number(m.seq) || 0;
    V.loading = false;
    render();
    onChange?.();
  }

  /** `agent_event`: one live event, in order (a number missing: open it again). */
  function onEvent(m) {
    if (!V.id || m.id !== V.id || !m.event) return;
    if (Number.isInteger(m.seq) && m.seq !== V.seq + 1 && !V.loading) {
      // Fell behind (or something went missing): a fresh snapshot.
      resync();
      return;
    }
    if (Number.isInteger(m.seq)) V.seq = m.seq;
    const ev = normEvent(m.event);
    if (Number.isInteger(m.line)) V.log.set(m.line, ev);
    else V.loose.push(ev);
    // The agent as it is now, as its tab on the PC keeps it (read again
    // when its status turns or a turn is new to it).
    const agent = V.agent;
    if (agent) {
      const was = agent.status;
      const next = { ...agent, status: ev.status ?? agent.status };
      if (ev.file && !(agent.files || []).includes(ev.file)) next.files = [...(agent.files || []), ev.file];
      if (ev.command) next.commands = [...(agent.commands || []), ev.command];
      if (typeof ev.cost === 'number') next.cost = ev.cost;
      if (ev.problem) next.problem = ev.problem;
      if (ev.file && Number.isInteger(ev.turn) && next.turnList?.length) {
        next.turnList = next.turnList.map((t) => (t.n === ev.turn && !(t.files || []).includes(ev.file) ? { ...t, files: [...(t.files || []), ev.file] } : t));
      }
      V.agent = next;
      const known = !Number.isInteger(ev.turn) || (next.turnList || []).some((t) => t.n === ev.turn);
      if (was !== next.status || !known || ev.queued) refreshSoon();
    }
    render();
    if (V.agent?.status !== agent?.status) onChange?.();
  }

  /** `agent_log`: an older page of its events. */
  function onLog(m) {
    if (m.id !== V.id) return;
    for (const x of m.events || []) if (Number.isInteger(x?.line)) V.log.set(x.line, normEvent(x.event));
    V.more = !!m.more;
    V.busy = V.busy === 'older' ? '' : V.busy;
    render();
  }

  /** `agent_diff`: the file asked for's lines. */
  function onDiff(m) {
    if (m.id !== V.id || !V.file || V.file.file !== m.file) return;
    V.file = { ...V.file, loading: false, lines: Array.isArray(m.lines) ? m.lines.map(String) : [], more: !!m.more, added: m.added, removed: m.removed };
    render();
  }

  /** `agent_resync`, a number missing, a reconnect: the snapshot again (what's here is kept, merged by line). */
  function resync() {
    if (!V.id) return;
    V.loading = true;
    send({ type: 'agent_open', id: V.id });
  }

  function refreshSoon() {
    clearTimeout(V.refreshTimer);
    V.refreshTimer = setTimeout(() => V.id && resync(), 400);
  }

  /** `acted`: a Code action done, in the PC's words. */
  function onActed(m) {
    if (!V.id || (m.id && m.id !== V.id)) return false;
    V.busy = '';
    V.error = '';
    V.note = String(m.message || '');
    if (m.action === 'undo') refreshSoon();
    render();
    onChange?.();
    return true;
  }

  /** The PC said no to this view's action: why, in the view. */
  function onError(message) {
    if (!V.id || !V.busy) return false;
    V.busy = '';
    V.error = message;
    render();
    onChange?.();
    return true;
  }

  // ---- actions ----

  function act(what, msg) {
    if (!V.agent || V.busy) return;
    V.note = '';
    V.error = '';
    if (!send(msg)) {
      V.error = `Not connected to ${pcName()}.`;
    } else V.busy = what;
    render();
    onChange?.();
  }

  /** "Tell the agent…": its next turn (queued while it works). False when it can't go. */
  function tell(text) {
    const t = String(text || '').trim();
    if (!t || !V.agent) return false;
    if (t.length > LIMITS.text) {
      V.error = "That's too long to send from the phone. Shorten it, then send it again.";
      render();
      return false;
    }
    if (V.busy) return false;
    act('tell', { type: 'agent_message', id: V.agent.id, text: t });
    return V.busy === 'tell';
  }

  const shortPath = (p) => String(p || '').split(/[\\/]/).pop() || String(p || '');

  /** Undo this turn (`turn`), or all: the phone's own confirm first. */
  function askUndo(turn) {
    const a = V.agent;
    if (!a) return;
    const t = turn != null ? turnsOf(a, events()).find((x) => x.n === turn) : null;
    const files = (t ? t.files : a.files) || [];
    V.undo = { turn };
    const n = files.length;
    els.undoTitle.textContent = `Put back ${n === 1 ? '1 file' : `${n} files`} in ${a.folder}?`;
    els.undoBody.textContent = t
      ? 'The files this turn changed go back to how they were before it, and the files it made are deleted. Yap kept their backups for this.'
      : 'Every file it changed goes back to how it was before it, and the files it made are deleted. Yap kept their backups for this.';
    els.undoFiles.replaceChildren(...files.map((f) => h('li', { text: shortPath(f), title: f })));
    els.undoGo.textContent = t ? 'Undo this turn' : 'Undo all';
    if (typeof els.undoSheet.showModal === 'function') {
      if (!els.undoSheet.open) els.undoSheet.showModal();
    } else els.undoSheet.setAttribute('open', '');
  }
  function closeUndo() {
    if (typeof els.undoSheet.close === 'function' && els.undoSheet.open) els.undoSheet.close();
    else els.undoSheet.removeAttribute('open');
  }
  els.undoGo.addEventListener('click', () => {
    const u = V.undo;
    closeUndo();
    if (!u || !V.agent) return;
    act('undo', { type: 'agent_undo', id: V.agent.id, ...(u.turn != null ? { turn: u.turn } : {}), sure: true });
  });
  els.undoCancel.addEventListener('click', closeUndo);
  els.undoSheet.addEventListener('click', (e) => e.target === els.undoSheet && closeUndo());

  els.close.addEventListener('click', close);

  // ---- following the live end ----

  els.body.addEventListener(
    'scroll',
    () => {
      const b = els.body;
      V.pinned = b.scrollHeight - b.scrollTop - b.clientHeight < 40;
      els.follow.hidden = V.pinned || !isLive(V.agent?.status);
    },
    { passive: true },
  );
  els.follow.addEventListener('click', () => {
    V.pinned = true;
    els.body.scrollTop = els.body.scrollHeight;
    els.follow.hidden = true;
  });
  // A finger on a button while events stream: the redraw waits for it to lift.
  els.root.addEventListener('pointerdown', () => (V.holding = true), { capture: true });
  const lift = () => {
    if (!V.holding) return;
    V.holding = false;
    if (V.dirty) setTimeout(render, 0);
  };
  els.root.addEventListener('pointerup', lift, { capture: true });
  els.root.addEventListener('pointercancel', lift, { capture: true });

  // ---- drawing ----

  let frame = 0;
  function render() {
    if (V.holding) {
      V.dirty = true;
      return;
    }
    if (frame) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      V.dirty = false;
      draw();
    });
  }

  function draw() {
    const a = V.agent;
    if (!a) return;
    const log = events();
    const live = isLive(a.status);
    const now = Date.now() / 1000;
    els.root.dataset.status = a.status || '';
    els.title.textContent = a.folder || 'Agent';
    els.title.title = a.path || a.folder || '';
    els.status.className = `avstatus tone-${statusTone(a.status)}`;
    els.status.replaceChildren(h('span', { class: 'adot', 'aria-hidden': 'true' }), statusWord(a.status) || 'Agent');

    // Its facts, its problem, what the PC last said.
    const turns = turnsOf(a, log);
    const many = turns.length > 1;
    const problem = problemWords(a.problem);
    fill(
      info,
      h('p', { class: 'avmeta', testid: 'agent-meta', text: metaLine(a, turns, now) }),
      a.path ? h('p', { class: 'avpath', text: a.path }) : null,
      h('p', { class: 'avcost', testid: 'agent-cost', data: { pay: a.pay || 'plan' }, text: `${costLine(a)}${many && Number(a.cost) > 0 ? ' in all' : ''}` }),
      problem
        ? h(
            'div',
            { class: 'avproblem', role: 'alert', testid: 'agent-problem' },
            h('p', { class: 'pline', text: problem.line }),
            problem.action === 'signIn'
              ? h('p', { class: 'pcheck', text: `Sign in to Claude Code on ${pcName()}, then press Continue.` })
              : problem.check
                ? h('p', { class: 'pcheck', text: problem.check })
                : null,
          )
        : a.status === 'failed' && a.error
          ? h('div', { class: 'avproblem', role: 'alert', testid: 'agent-problem' }, h('p', { class: 'pline', text: a.error }))
          : null,
      V.loading && !log.length ? h('p', { class: 'avloading', text: 'Asking your PC for its steps…' }) : null,
    );

    // Its cards, in place.
    cards.mount(cardsRoot, 'agent', (c) => c.kind === 'approve' && c.run === a.id);

    // The turns.
    const waiting = (t) =>
      !t.startedTs &&
      t.n !== turns[0]?.n &&
      (t.status === 'queued' ? turns.some((o) => o.n !== t.n && (o.status === 'running' || o.status === 'asking')) : t.status === 'stopped' || t.status === 'interrupted');
    const current = [...turns].reverse().find((t) => !waiting(t))?.n ?? turns.at(-1)?.n;
    const frag = document.createDocumentFragment();
    // Older steps, before the first shown.
    if (V.more) {
      frag.append(
        h('button', {
          type: 'button',
          class: 'avolder',
          testid: 'agent-older',
          disabled: V.busy === 'older',
          text: V.busy === 'older' ? 'Asking for older steps…' : 'Older steps',
          on: { click: older },
        }),
      );
    }
    for (const t of turns) {
      const isQueued = waiting(t);
      const open = !isQueued && (V.folded[t.n] ?? t.n !== current) === false;
      const sec = h('section', { class: `avturn${open ? ' open' : ''}${isQueued ? ' queued' : ''}`, testid: 'agent-turn', data: { turn: t.n, status: t.status ?? '', open: String(open) } });
      if (isQueued) {
        sec.append(
          h('div', { class: 'thead' }, h('span', { class: 'tdot tone-idle', 'aria-hidden': 'true' }), h('span', { class: 'ttitle', text: `Queued: ${t.title || 'your next request'}` })),
          h('p', { class: 'tqueued', text: t.status === 'queued' ? 'It goes to the agent once it finishes this step.' : 'Not sent: the agent stopped first. Continue sends it.' }),
        );
      } else {
        sec.append(
          h(
            'button',
            { type: 'button', class: 'thead', 'aria-expanded': String(open), testid: 'agent-turn-head', on: { click: () => ((V.folded = { ...V.folded, [t.n]: open }), render()) } },
            svg(ICONS.chevron, `tchev${open ? ' down' : ''}`),
            h('span', { class: `tdot tone-${statusTone(t.status)}`, 'aria-hidden': 'true' }),
            h('span', { class: 'ttitle', text: t.title || a.folder }),
            h('span', { class: 'tfacts', text: turnFacts(a, t, now) }),
          ),
        );
        if (open) sec.append(turnBody(a, t, live && t.n === current, !many, many && canUndoTurn(a, t)));
      }
      frag.append(sec);
    }
    if (V.note) frag.append(h('p', { class: 'avnote', role: 'status', testid: 'agent-note', text: V.note }));
    if (V.error) frag.append(h('p', { class: 'avnote bad', role: 'alert', testid: 'agent-error', text: V.error }));
    // A diff scrolled sideways (or down) stays where it was.
    const code = turnsRoot.querySelector('.dcode');
    const kept = code ? [code.scrollLeft, code.scrollTop] : null;
    turnsRoot.replaceChildren(frag);
    const again = kept && turnsRoot.querySelector('.dcode');
    if (again) [again.scrollLeft, again.scrollTop] = kept;
    drawFoot(a, live, many, turns);
    els.follow.hidden = V.pinned || !live;
    if (V.pinned) els.body.scrollTop = els.body.scrollHeight;
    // A turn asked for (a pill's, a report's): in view, once — unless it's
    // the latest, where following the live end shows it best.
    if (V.asked != null && !V.loading) {
      const el = V.asked !== turns.at(-1)?.n ? turnsRoot.querySelector(`[data-turn="${V.asked}"]`) : null;
      V.asked = null;
      if (el) {
        V.pinned = false;
        el.scrollIntoView?.({ block: 'start' });
      }
    }
  }

  /** "Auto · Claude Sonnet 5.5 · 4 min so far · 2 turns". */
  function metaLine(a, turns, now) {
    const ran = workedFor(a, now);
    const live = isLive(a.status);
    return [modeLabel(a.mode), a.model ? modelLabel(a.model) : '', ran ? (live ? `${ran} so far` : `ran ${ran}`) : '', turns.length > 1 ? `${turns.length} turns` : ''].filter(Boolean).join(' · ');
  }

  /** The times only (each second while it works): the meta line and each turn's facts. */
  function drawTimes() {
    const a = V.agent;
    if (!a) return;
    const now = Date.now() / 1000;
    const turns = turnsOf(a, events());
    const meta = info.querySelector('.avmeta');
    if (meta) meta.textContent = metaLine(a, turns, now);
    for (const t of turns) {
      const f = turnsRoot.querySelector(`[data-turn="${t.n}"] .tfacts`);
      if (f) f.textContent = turnFacts(a, t, now);
    }
  }

  function turnFacts(a, t, now) {
    const parts = [turnStatusWord(t)];
    const took = t.startedTs ? elapsed(t, now) : '';
    if (took) parts.push(isLive(t.status) ? `${took} so far` : took);
    if (Number(t.cost) > 0) parts.push(a.pay === 'key' ? money(t.cost) : `≈ ${money(t.cost)}`);
    return parts.join(' · ');
  }

  /** One turn's body (AgentTurn.svelte's parts). */
  function turnBody(a, t, live, single, undoable) {
    const ev = t.events || [];
    const body = h('div', { class: 'tbody' });
    const latest = live ? latestStep(ev, a.status) : '';
    if (latest) body.append(h('p', { class: 'avnow', testid: 'agent-now', 'aria-live': 'polite' }, h('span', { class: 'spin', 'aria-hidden': 'true' }), latest));
    const todos = todosOf(ev);
    if (todos.length) {
      body.append(
        h(
          'div',
          { class: 'avsec', testid: 'agent-todos' },
          h('h3', {}, 'To-do ', h('span', { class: 'cnt', text: todoCount(todos) })),
          h(
            'ul',
            { class: 'todolist' },
            todos.map((td) =>
              h(
                'li',
                { class: `todo ${td.status}`, data: { status: td.status } },
                h('span', { class: 'tmark', 'aria-hidden': 'true', text: td.status === 'completed' ? '✓' : td.status === 'in_progress' ? '●' : '○' }),
                h('span', { class: 'ttext', text: td.status === 'in_progress' && td.activeForm ? td.activeForm : td.content }),
                h('span', { class: 'vh', text: td.status === 'completed' ? ', done' : td.status === 'in_progress' ? ', in progress' : ', to do' }),
              ),
            ),
          ),
        ),
      );
    }
    const timeline = timelineOf(ev, live ? '' : t.text);
    if (timeline.length) {
      const steps = timeline.filter((x) => x.kind === 'step').length;
      body.append(
        h(
          'div',
          { class: 'avsec', testid: 'agent-steps' },
          h('h3', {}, 'Steps ', h('span', { class: 'cnt', text: String(steps) })),
          h(
            'ol',
            { class: 'timeline' },
            timeline.map((x, i) => {
              if (x.kind === 'said') {
                const key = `${t.n}:${i}`;
                const folds = longSaid(x.text);
                const open = V.unfolded.has(key);
                return h(
                  'li',
                  { class: 'said', testid: 'agent-said' },
                  h('p', { class: `saidtext${folds && !open ? ' folded' : ''}`, text: x.text }),
                  folds
                    ? h('button', {
                        type: 'button',
                        class: 'showmore',
                        'aria-expanded': String(open),
                        text: open ? 'Show less' : 'Show more',
                        on: { click: () => (open ? V.unfolded.delete(key) : V.unfolded.add(key), render()) },
                      })
                    : null,
                );
              }
              return h(
                'li',
                { class: 'step', testid: 'agent-step' },
                h('span', { class: 'sdotline', 'aria-hidden': 'true' }),
                h('span', { class: 'steptext' }, h('span', { text: x.step }), x.about ? h('span', { class: 'stepabout', text: x.about }) : null),
              );
            }),
          ),
        ),
      );
    }
    if (t.files?.length) {
      const diffs = diffsOf(ev);
      const list = h('ul', { class: 'files' });
      for (const f of t.files) {
        const facts = fileFacts(diffs.get(f));
        const opened = V.file && V.file.file === f && V.file.turn === t.n;
        list.append(
          h(
            'li',
            { class: `fileitem${opened ? ' open' : ''}` },
            h(
              'button',
              { type: 'button', class: 'filerow', testid: 'agent-file', 'aria-expanded': String(!!opened), data: { file: f }, on: { click: () => toggleFile(f, t.n) } },
              svg(ICONS.file, 'ficon'),
              h('span', { class: 'fname', text: f }),
              facts.title ? h('span', { class: 'fkind', text: facts.title }) : null,
              counts(facts),
            ),
            opened ? renderDiff({ ...V.file, onWrap: () => ((V.file = { ...V.file, wrap: !V.file.wrap }), render()) }) : null,
          ),
        );
      }
      body.append(
        h(
          'div',
          { class: 'avsec', testid: 'agent-files' },
          h('h3', {}, 'Files changed ', h('span', { class: 'cnt', text: String(t.files.length) })),
          list,
          a.noBackups ? h('p', { class: 'avhint', text: NO_BACKUPS }) : null,
          undoable && !V.busy ? h('button', { type: 'button', class: 'pillbtn', testid: 'agent-undo-turn', text: 'Undo this turn', on: { click: () => askUndo(t.n) } }) : null,
        ),
      );
    }
    const told = ev.filter((e) => typeof e?.command === 'string');
    const commands = told.length || !single ? commandsOf({ commands: told.map((e) => e.command) }, told) : commandsOf(a, ev);
    if (commands.length) {
      const all = !!V.allCommands[t.n];
      body.append(
        h(
          'div',
          { class: 'avsec', testid: 'agent-commands' },
          h('h3', {}, 'Commands run ', h('span', { class: 'cnt', text: String(commands.length) })),
          h(
            'ul',
            { class: 'cmds' },
            (all ? commands : commands.slice(0, FEW_COMMANDS)).map((c) =>
              h('li', { testid: 'agent-command' }, c.about ? h('p', { class: 'cabout', text: c.about }) : null, h('code', { class: 'cmd', text: c.command })),
            ),
          ),
          commands.length > FEW_COMMANDS
            ? h('button', {
                type: 'button',
                class: 'showmore',
                'aria-expanded': String(all),
                text: all ? 'Show fewer' : `Show ${commands.length - FEW_COMMANDS} more`,
                on: { click: () => ((V.allCommands = { ...V.allCommands, [t.n]: !all }), render()) },
              })
            : null,
        ),
      );
    }
    if (t.denials?.length) {
      body.append(h('div', { class: 'avsec', testid: 'agent-denials' }, h('h3', { text: "What wasn't allowed" }), h('ul', { class: 'plain' }, t.denials.map((d) => h('li', { text: d.detail || d.tool })))));
    }
    if (!live && t.text) {
      body.append(h('div', { class: 'avsec final', testid: 'agent-text' }, h('h3', { text: t.plan ? 'Its plan' : 'What it said' }), h('div', { class: 'md' }, renderAnswer(t.text))));
    }
    if (t.undone) body.append(h('p', { class: 'avhint', text: 'Undone: its files are back as they were.' }));
    return body;
  }

  let footKey = '';
  function drawFoot(a, live, many, turns) {
    const last = turns.at(-1) || null;
    const goAhead = planWaits(a, last, true) && !(a.id in V.wentAhead);
    const stopped = a.status === 'stopped' || a.status === 'interrupted' || a.status === 'failed';
    const undo = canUndo(a);
    const key = [a.id, live, stopped, goAhead, undo, many, V.busy].join('|');
    if (key === footKey) return;
    footKey = key;
    const busy = !!V.busy;
    const btns = [];
    if (live) btns.push(h('button', { type: 'button', class: 'pillbtn', testid: 'agent-stop', disabled: busy, text: V.busy === 'stop' ? 'Stopping…' : 'Stop', on: { click: () => act('stop', { type: 'agent_stop', id: a.id }) } }));
    else if (stopped) btns.push(h('button', { type: 'button', class: 'pillbtn primary', testid: 'agent-continue', disabled: busy, text: 'Continue', on: { click: () => act('continue', { type: 'agent_continue', id: a.id }) } }));
    if (goAhead) {
      btns.push(
        h('button', {
          type: 'button',
          class: 'pillbtn primary',
          testid: 'agent-go-ahead',
          disabled: busy,
          text: 'Go ahead',
          on: {
            click: () => {
              V.wentAhead[a.id] = last?.n ?? 0;
              act('go_ahead', { type: 'agent_go_ahead', id: a.id });
            },
          },
        }),
      );
    }
    if (undo) btns.push(h('button', { type: 'button', class: 'pillbtn', testid: 'agent-undo', disabled: busy, text: many ? 'Undo all' : "Undo the agent's changes", on: { click: () => askUndo(null) } }));
    els.foot.replaceChildren(...btns);
    els.foot.hidden = btns.length === 0;
  }

  function older() {
    const first = [...V.log.keys()].sort((x, y) => x - y)[0];
    if (!V.id || first == null || V.busy) return;
    V.busy = 'older';
    send({ type: 'agent_log', id: V.id, before: first });
    render();
  }

  /** A file's diff, one at a time: asked for; tapped again, closed. */
  function toggleFile(file, turn) {
    if (V.file && V.file.file === file && V.file.turn === turn) V.file = null;
    else {
      V.file = { file, turn, loading: true, lines: [], more: false, wrap: V.file?.wrap ?? false };
      send({ type: 'agent_diff', id: V.id, file, turn });
    }
    render();
  }

  return {
    open,
    close,
    onSnapshot,
    onEvent,
    onLog,
    onDiff,
    onActed,
    onError,
    tell,
    resync,
    render,
    /** Back to following the live end (a card of its came: it's there). */
    follow() {
      V.pinned = true;
      render();
    },
    get id() {
      return isOpen() ? V.id : '';
    },
    get busy() {
      return V.busy;
    },
    get agent() {
      return V.agent;
    },
    /** Remote Control went off, or the chat went: closed without a word to the PC. */
    drop() {
      V.id = '';
      clearInterval(V.tick);
      els.root.hidden = true;
      closeUndo();
    },
    /** The tests' view. */
    snapshot: () => (V.id ? { id: V.id, status: V.agent?.status || '', events: V.log.size + V.loose.length, seq: V.seq, more: V.more, file: V.file ? { file: V.file.file, lines: V.file.lines.length, wrap: !!V.file.wrap } : null, busy: V.busy, pinned: V.pinned } : null),
  };
}
