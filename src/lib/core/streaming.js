// Streamed answers (src-tauri/src/chat/stream.rs): Chat and the notepad's Ask
// sheet show an answer as it's written instead of after it's done.
//
// - `streamCall(cmd, args, onEvent)` invokes a command with a Tauri channel
//   (`onStream`) and a stop id (`turnId`): each event the backend streams
//   reaches `onEvent`, `stop()` is the ■ (the stream closes; a model on this
//   PC stops generating), `supersede()` drops it for a newer question the
//   page sends next (nothing saved: the promise fails with SUPERSEDED), and
//   the promise ends with the command's own answer — the CHECKED one
//   (honesty checks: no made-up sources list, no claimed search, no
//   citation with nothing behind it), which the page puts in place of what
//   streamed. A newer question in the same conversation from anywhere (the
//   Voice Assistant) ends it with SUPERSEDED too (chat/stream.rs `Halt`).
// - `liveAnswer()` / `applyEvent()`: the streamed bubble's state from the
//   events (`text`: the answer so far, whole; `tool`: a tool call began —
//   its chip "Searching the web…"; `chips`: the tools done and the web
//   sources found so far; `waiting`: a model on this PC stepped aside for a
//   dictation).
// - `closePartial(md)`: markdown still being written, closed off so it
//   renders as it will (an open code fence, a half-written **bold** or
//   `code` on the last line, a half-typed citation or link left out).
// - `settle` / `streamingHeight`: when the checked answer replaces the
//   streamed one and differs, the bubble glides from the streamed text's
//   height to the new one and the text fades in, instead of jumping.
import { Channel, invoke } from '@tauri-apps/api/core';
import { motionReduced } from './appearance.js';

let seq = 0;

/** What a stopped answer with nothing shown yet comes back as (chat/stream.rs STOPPED). */
export const STOPPED = 'Stopped before the answer began.';

/**
 * What a turn a newer question in its conversation replaced comes back as
 * (chat/stream.rs SUPERSEDED): no answer saved, the question kept.
 */
export const SUPERSEDED = 'A newer question replaced this one.';

/**
 * Invoke `cmd` streamed: `onEvent(ev)` hears each event, `stop()` ends it
 * (what came is kept), `supersede()` drops it for a newer question.
 * Returns `{ promise, stop, supersede, turnId }`.
 */
export function streamCall(cmd, args, onEvent) {
  const turnId = `t${Date.now().toString(36)}-${++seq}`;
  const channel = new Channel();
  channel.onmessage = (ev) => {
    try {
      onEvent(ev);
    } catch {
      /* a page gone meanwhile */
    }
  };
  const promise = invoke(cmd, { ...args, onStream: channel, turnId });
  const stop = () => invoke('stream_stop', { id: turnId }).catch(() => false);
  const supersede = () => invoke('stream_stop', { id: turnId, superseded: true }).catch(() => false);
  return { promise, stop, supersede, turnId };
}

/**
 * A streamed answer's state before anything came. `thinking`: a question
 * asked to think it through (chat/think.rs), its thinking so far.
 */
export function liveAnswer() {
  return { text: '', chips: [], doing: null, sources: [], web: false, waiting: false, started: false, thinking: '' };
}

/**
 * `state` after `ev` (a new object). `started`: something to show (text or
 * a tool call) has come, so "Thinking…" can go.
 */
export function applyEvent(state, ev) {
  const s = { ...state };
  switch (ev?.kind) {
    case 'text':
      s.text = ev.text ?? '';
      if (s.text) {
        s.started = true;
        s.waiting = false;
      }
      break;
    case 'tool':
      s.doing = ev.doing || 'Working…';
      s.started = true;
      break;
    case 'chips':
      s.chips = ev.chips ?? [];
      s.sources = ev.sources ?? [];
      s.web = !!ev.web;
      s.doing = null;
      s.started = true;
      break;
    case 'waiting':
      s.waiting = !!ev.waiting;
      break;
    case 'thinking':
      // Its thinking so far, whole (shown folded, apart from the answer).
      s.thinking = ev.text ?? '';
      break;
    default:
      break;
  }
  return s;
}

/**
 * Markdown still being written, closed off so it renders as it will. Pure.
 * - An open ``` fence is closed (the code shows as code while it comes).
 * - On the last line: a half-typed citation "[1" or link "[text](https://ex"
 *   is left out until it's whole; an odd ` or ** is closed.
 */
export function closePartial(md) {
  const text = md ?? '';
  const fences = (text.match(/^\s*(```|~~~)/gm) || []).length;
  if (fences % 2 === 1) return `${text}\n\`\`\``;
  const nl = text.lastIndexOf('\n');
  const head = text.slice(0, nl + 1);
  let last = text.slice(nl + 1);
  // A link whose address is still coming, then a bracket not yet closed.
  last = last.replace(/\[[^\]\n]*\]\([^)\s]*$/, '').replace(/\[[^\]\n]*$/, '');
  if ((last.match(/`/g) || []).length % 2 === 1) last = `${last.replace(/\s+$/, '')}\``;
  if ((last.match(/\*\*/g) || []).length % 2 === 1) last = `${last.replace(/\s+$/, '')}**`;
  return head + last;
}


/**
 * The height of the answer being written in `root` (its bubble has
 * `.streaming`), before the checked answer replaces it: what `settle`
 * glides from. 0 when there's none.
 */
export function streamingHeight(root) {
  return root?.querySelector?.('.streaming')?.getBoundingClientRect().height ?? 0;
}

/**
 * Svelte action: `use:settle={from}` on an answer's bubble. `from` is the
 * streamed answer's height when the checked answer, different from it,
 * took its place (else 0): the bubble glides from that height to its new
 * one and its text fades in, instead of jumping. Nothing while motion is
 * reduced.
 */
export function settle(node, from) {
  let done = 0;
  function swap(h) {
    if (!h || h === done) return;
    done = h;
    if (motionReduced() || !node.animate) return;
    const to = node.getBoundingClientRect().height;
    node.style.overflow = 'hidden';
    const glide = node.animate([{ height: `${h}px` }, { height: `${to}px` }], { duration: 220, easing: 'ease-out' });
    node.animate([{ opacity: 0.45 }, { opacity: 1 }], { duration: 260, easing: 'ease-out' });
    glide.onfinish = glide.oncancel = () => (node.style.overflow = '');
  }
  swap(from);
  return { update: swap };
}
