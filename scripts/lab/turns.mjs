// What a turn did, from the test Yap's record (desktop/src-tauri/src/shell/lab.rs:
// `stream`, `sent`, `tool`, `claude` and `event` items) and the conversation
// as Chat saved it. Pure: the lab's daemon reads, this sums up (docs/lab.md).

/** The longest a string field is in `ask`'s JSON (the transcript file has it whole). */
export const FIELD_MAX = 400;
/** The longest an answer is in `ask`'s JSON. */
export const ANSWER_MAX = 4_000;

/**
 * `s` cut to `max` characters, its full length said when it was:
 * `"…first part… […+1234 chars, 1634 in all]"`. Pure.
 */
export function cut(s, max = FIELD_MAX) {
  const text = String(s ?? '');
  if (text.length <= max) return text;
  return `${text.slice(0, max)}… [+${text.length - max} chars, ${text.length} in all]`;
}

/** A value as one short line: a string as it is, anything else as JSON, cut. Pure. */
export function brief(v, max = FIELD_MAX) {
  if (v === undefined || v === null) return '';
  return cut(typeof v === 'string' ? v : JSON.stringify(v), max);
}

/** Words that say a tool was refused (not merely failed). */
// (A heuristic: `ok: false` is what the tool said; `refused` is a guess
// that it was one of Yap's rules rather than a failure.)
const REFUSAL = /\b(refus|not allowed|isn't allowed|aren't allowed|you said no|said no|denied|deny|won't|never (?:types?|clicks?|opens?|goes|reads?)|not permitted|blocked|asks? the user first|needs the user|not a link from you|only takes|only opens|isn't offered|not offered|off for)/i;

/**
 * The stream events of a turn, typed or spoken, in order: `{ seq, ts, ev }`
 * (`ev`: the Chat stream's `{kind, …}`). Pure.
 */
export function streamEvents(items) {
  const out = [];
  for (const it of items) {
    if (it.kind === 'stream' && it.event) out.push({ seq: it.seq, ts: it.ts, ev: it.event });
    else if (it.kind === 'event' && it.name === 'yap-assistant-stream' && it.payload?.event) out.push({ seq: it.seq, ts: it.ts, ev: it.payload.event, conversation: it.payload.conversationId });
  }
  return out;
}

/** The conversation a turn went to, once the record says. Pure. */
export function conversationOf(items) {
  for (const it of items) {
    if (it.kind === 'stream' && it.event?.kind === 'turn' && it.event.conversationId != null) return it.event.conversationId;
    if (it.kind === 'sent' && it.ok && it.reply?.conversationId != null) return it.reply.conversationId;
    if (it.kind === 'event' && it.name === 'yap-assistant-turn' && it.payload?.conversationId != null && ['asked', 'answered', 'failed'].includes(it.payload.phase)) return it.payload.conversationId;
  }
  return null;
}

/**
 * Has the turn ended, and how: `{ done, status, error }` — status
 * "answered", "stopped" or "failed". A typed turn ends with `chat_send`'s
 * answer (`sent`); a spoken one with the Voice Assistant's `answered` or
 * `failed`, or its "couldn't answer" / "no AI model" alert. Pure.
 */
export function endOf(items, { spoken = false } = {}) {
  for (const it of items) {
    if (!spoken && it.kind === 'sent') {
      if (it.ok) return { done: true, status: it.reply?.stopped ? 'stopped' : 'answered', error: null };
      const error = String(it.error ?? 'failed');
      return { done: true, status: /stopped/i.test(error) ? 'stopped' : 'failed', error };
    }
    if (spoken && it.kind === 'event') {
      const p = it.payload ?? {};
      if (it.name === 'yap-assistant-turn' && p.phase === 'answered') return { done: true, status: 'answered', error: null };
      if (it.name === 'yap-assistant-turn' && p.phase === 'failed') return { done: true, status: 'failed', error: String(p.error ?? 'failed') };
      if (it.name === 'yap-assistant-turn' && p.phase === 'superseded') return { done: true, status: 'failed', error: 'a newer question replaced it' };
      if (it.name === 'yap-alert' && /^assistant(NoModel|Failed)$/.test(p.kind ?? '')) return { done: true, status: 'failed', error: String(p.body ?? p.title ?? p.kind) };
    }
  }
  return { done: false, status: 'running', error: null };
}

/**
 * The cards the turn showed (a page action, an agent, a connector: Chat's
 * "Yap wants to…" cards), each as it is now: `{ id: "confirm:<n>", act,
 * label, site, does, state, … }`. `approvals`: Claude Code's permission
 * prompts waiting now (`claude_approve_pending`), as `approve:<n>`;
 * `agents`: Yap Code's agents (`agent_list`), a plan waiting for Go ahead
 * as `plan:<id>`. Pure.
 */
export function cardsOf(items, { approvals = [], agents = [] } = {}) {
  const cards = new Map();
  for (const { ev } of streamEvents(items)) {
    if (ev.kind !== 'confirm' || ev.id == null) continue;
    const card = {
      id: `confirm:${ev.id}`,
      kind: ev.act === 'connector' ? 'connector' : ev.act === 'agent' ? 'agent' : 'page',
      act: ev.act || 'press',
      label: ev.label ?? '',
      site: ev.site ?? '',
      does: ev.does ?? '',
      state: ev.state ?? '',
      answers: ev.act === 'connector' ? ['allow', 'always', 'deny'] : ['allow', 'deny'],
    };
    if (ev.text) card.text = cut(ev.text, 200);
    if (ev.agent) card.agent = { folder: ev.agent.folder ?? ev.agent.name ?? '', mode: ev.agent.mode ?? '', brief: cut(ev.agent.brief ?? '', 300) };
    if (ev.connector) card.connector = { connector: ev.connector.connector ?? '', tool: ev.connector.tool ?? '', args: (ev.connector.args ?? []).map((a) => `${a.key}: ${a.value}`) };
    cards.set(card.id, card);
  }
  for (const p of approvals) {
    cards.set(`approve:${p.id}`, { id: `approve:${p.id}`, kind: 'approve', act: 'approve', tool: p.tool, label: p.summary ?? '', detail: cut(p.detail ?? '', 300), run: p.run, state: 'asking', answers: ['allow', 'deny'] });
  }
  for (const a of agents) {
    // (A plan set aside — "Plan not used": a newer turn began in its chat, or
    // it was put aside — waits on nobody, as the Agent panel has it.)
    if (a.plan && !a.planAllowed && !a.planUnused && !['queued', 'running', 'asking'].includes(String(a.status).toLowerCase())) {
      cards.set(`plan:${a.id}`, { id: `plan:${a.id}`, kind: 'plan', act: 'go ahead', label: `${a.folder}: Go ahead with the plan?`, detail: cut(a.text ?? '', 300), state: 'asking', answers: ['allow', 'deny'] });
    }
  }
  return [...cards.values()];
}

/** The cards still waiting for an answer. Pure. */
export const pendingOf = (cards) => cards.filter((c) => c.state === 'asking');

/** Whether a Yap tool result says it was refused. Pure. */
function yapRefused(result) {
  if (!result || result.success !== false) return false;
  const said = [result.displayText, result.error, result.message, result.data?.reason].filter(Boolean).join(' ');
  return REFUSAL.test(said) || result.data?.allowed === false || result.data?.refused === true;
}

/** A Yap tool result, short: its chip, its error, its data cut. Pure. */
function yapResult(result) {
  if (result === null || typeof result !== 'object') return brief(result);
  const parts = [];
  if (result.displayText) parts.push(String(result.displayText));
  if (result.error) parts.push(`error: ${result.error}`);
  if (result.message && result.message !== result.displayText) parts.push(String(result.message));
  if (result.data !== undefined) parts.push(`data: ${brief(result.data, 600)}`);
  return cut(parts.join(' · ') || JSON.stringify(result), 800);
}

/** A Claude Code tool_result's content as text (pictures counted). Pure. */
function claudeContent(content) {
  if (typeof content === 'string') return content;
  if (!Array.isArray(content)) return content == null ? '' : JSON.stringify(content);
  const texts = [];
  let pictures = 0;
  for (const b of content) {
    if (b?.type === 'text') texts.push(b.text ?? '');
    else if (b?.type === 'image') pictures++;
    else texts.push(JSON.stringify(b));
  }
  if (pictures) texts.push(`[${pictures} picture${pictures > 1 ? 's' : ''}]`);
  return texts.join('\n');
}

/**
 * Every tool call of the turn, in order: Yap's tool loop's (`via: "yap"`)
 * and Claude Code's (`via: "claude"`: its own tools, and Yap's MCP tools as
 * `server: "yap"` with Yap's name), each `{ name, args, result, ok, refused }`
 * — trimmed (`full`: whole, for the transcript). An agent at work while
 * the turn ran has its own Claude Code, whose calls land in the same stretch
 * of the record: those are `via: "agent"` (the record's `agent`), never
 * the Chat turn's (11 Nov 2026: a note to a working agent listed the agent's
 * Read and Edit as Chat's). Pure.
 */
export function toolsOf(items, { full = false } = {}) {
  const max = full ? Infinity : FIELD_MAX;
  const out = [];
  const byUse = new Map();
  for (const it of items) {
    if (it.kind === 'tool') {
      const refused = yapRefused(it.result);
      out.push({
        via: 'yap',
        name: it.name,
        args: full ? it.args : brief(it.args, max),
        result: full ? it.result : yapResult(it.result),
        ok: it.result?.success !== false,
        ...(refused ? { refused: true } : {}),
        seq: it.seq,
      });
      continue;
    }
    if (it.kind !== 'claude') continue;
    const line = it.line ?? {};
    if (line.type === 'assistant') {
      for (const b of line.message?.content ?? []) {
        if (b?.type !== 'tool_use' && b?.type !== 'server_tool_use') continue;
        const mcp = /^mcp__([^_]+(?:_[^_]+)*)__(.+)$/.exec(b.name ?? '');
        const call = {
          via: it.agent ? 'agent' : 'claude',
          ...(mcp ? { server: mcp[1] } : {}),
          name: mcp ? mcp[2] : b.name,
          args: full ? b.input : brief(b.input, max),
          result: null,
          ok: null,
          seq: it.seq,
          pid: it.pid,
        };
        byUse.set(b.id, call);
        out.push(call);
      }
    } else if (line.type === 'user') {
      const content = line.message?.content;
      for (const b of Array.isArray(content) ? content : []) {
        if (b?.type !== 'tool_result') continue;
        const call = byUse.get(b.tool_use_id);
        if (!call) continue;
        const text = claudeContent(b.content);
        call.result = full ? b.content : cut(text, 800);
        call.ok = !b.is_error;
        if (b.is_error && REFUSAL.test(text)) call.refused = true;
        // Yap's own tools answer refusals as text (a "success": false JSON).
        if (!b.is_error && /"success"\s*:\s*false/.test(text)) {
          call.ok = false;
          if (REFUSAL.test(text)) call.refused = true;
        }
      }
    } else if (line.type === 'system' && line.subtype === 'permission_denied') {
      out.push({ via: it.agent ? 'agent' : 'claude', name: line.tool_name ?? line.tool ?? 'tool', args: brief(line.tool_input ?? line.input, max), result: brief(line.message ?? line.reason ?? 'permission denied'), ok: false, refused: true, seq: it.seq, pid: it.pid });
    }
  }
  return out;
}

/** The last answer of conversation `conv` (Chat's saved one), after message `fromIndex`. Pure. */
export function answerOf(conv, fromIndex = 0) {
  const messages = conv?.messages ?? [];
  for (let i = messages.length - 1; i >= Math.max(0, fromIndex); i--) {
    if (messages[i].role === 'assistant' && messages[i].kind !== 'agentReport') return { index: i, message: messages[i] };
  }
  return null;
}

/** The question a turn asked in conversation `conv`: its first person's
 * message from `fromIndex` on (Chat's saved one). Pure. */
export function questionOf(conv, fromIndex = 0) {
  const messages = conv?.messages ?? [];
  for (let i = Math.max(0, fromIndex); i < messages.length; i++) {
    if (messages[i].role === 'user') return messages[i];
  }
  return null;
}

/**
 * **`ask`'s JSON**: what the turn did. `turn`: `{ n, question, spoken,
 * startedAt, endedAt?, status, error, conversation, askedAt (the
 * conversation's length when asked) }`; `items`: its part of the record;
 * `conv`: the conversation now (`chat_get`); `pages`: the pages alive
 * (`e2e_page_views`); `approvals`, `agents`: as for [`cardsOf`]. Pure.
 */
/** Every item an answer shared (`files`: shares, each with its `items`), the
 * share's caption on each. Older shapes (a flat list) read as items. Pure. */
export function sharedItems(message) {
  return (message?.files ?? []).flatMap((s) => (Array.isArray(s?.items) ? s.items.map((i) => ({ ...i, caption: s.caption ?? '' })) : [s]));
}

export function summarize({ turn, items, conv, pages = [], approvals = [], agents = [], transcript = null, now = Date.now() }) {
  const found = answerOf(conv, turn.askedAt ?? 0);
  const message = found?.message ?? null;
  const sent = items.find((it) => it.kind === 'sent' && it.ok);
  const answer = message?.text ?? sent?.reply?.reply ?? '';
  const lastChips = [...streamEvents(items)].reverse().find((e) => e.ev.kind === 'chips')?.ev.chips;
  const steps = message?.tools?.length ? message.tools : lastChips ?? sent?.reply?.toolsUsed ?? [];
  const cards = cardsOf(items, { approvals, agents });
  const pending = pendingOf(cards);
  const shownPages = streamEvents(items).filter((e) => e.ev.kind === 'page').map((e) => e.ev.page?.url).filter(Boolean);
  const out = {
    turn: turn.n,
    status: pending.length && !['answered', 'stopped', 'failed'].includes(turn.status) ? 'waiting' : turn.status,
    conversation: turn.conversation ?? conversationOf(items),
    question: cut(turn.question, 300),
    spoken: !!turn.spoken,
    // The screenshots kept with the question itself: a snip, the window in
    // front its words took, the model's own look at the screen ("window" or
    // "screen"), each by its source.
    questionImages: (questionOf(conv, turn.askedAt ?? 0)?.images ?? []).map((i) => ({ source: i.source ?? '', asText: !!i.asText })),
    answer: cut(answer, ANSWER_MAX),
    model: message?.model ?? sent?.reply?.model ?? null,
    steps,
    // The turn's own calls; an agent's at work meanwhile, by name only.
    tools: toolsOf(items).filter((t) => t.via !== 'agent').map(({ seq, pid, ...t }) => t),
    agentTools: toolsOf(items).filter((t) => t.via === 'agent').map((t) => t.name),
    cards,
    pending: pending.map((c) => c.id),
    pages: pages.map((p) => ({ url: p.url ?? p.address ?? '', onScreen: p.onScreen ?? p.on_screen ?? null, loaded: p.loaded ?? null })),
    pagesShown: [...new Set([...(message?.pages ?? []).map((p) => p.url), ...shownPages])],
    // Pictures the answer shows: the web's (`pictures`, show_picture) and the
    // ones `share_files` put inline (an answer's `files` are shares, each with
    // `items`: chat/shares.rs; a picture item has its chat-images `id`).
    pictures: [
      ...(message?.pictures ?? []).map((p) => ({ caption: cut(p.caption ?? '', 120), url: p.page?.url ?? p.url ?? '' })),
      ...sharedItems(message).filter((f) => f.kind === 'image' && f.id).map((f) => ({ caption: cut(f.caption ?? '', 120), url: f.imageUrl ?? '', name: f.name ?? '', shared: true })),
    ],
    // Files shared as cards (`share_files`, or an agent's report): every
    // shared item that isn't a picture shown inline.
    files: sharedItems(message).filter((f) => !(f.kind === 'image' && f.id)).map((f) => ({ name: f.name ?? String(f.path ?? '').split(/[\\/]/).pop() ?? '', path: f.path ?? '', kind: f.kind ?? '' })),
    documents: (message?.documents ?? []).map((d) => d.title ?? d.noteId ?? d),
    sources: (message?.sources ?? sent?.reply?.sources ?? []).map((s) => s.url).filter(Boolean),
    error: turn.error ?? null,
    ms: (turn.endedAt ?? now) - turn.startedAt,
    transcript,
  };
  if (out.status === 'waiting') out.hint = `A card is waiting (it times out after 2 minutes): npm run lab -- answer ${pending[0].id} allow|deny${pending[0].answers.includes('always') ? '|always' : ''}`;
  if (out.status === 'running') out.hint = 'Still answering: npm run lab -- wait';
  return out;
}
