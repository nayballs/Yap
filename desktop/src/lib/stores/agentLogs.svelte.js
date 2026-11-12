// **Agents' logs for the thread** (views/chat/AgentWork.svelte): each watched
// agent's `yap-agent` events, oldest first — read once (`agent_get`, as its
// tab reads them) and kept in step with the app-wide event from then on. The
// agents store (agents.svelte.js) keeps each agent's record and the line it's
// on, never its log; the Agent tab keeps the open agent's for itself. A Code
// chat's thread shows what its agents said and did as they work (11 Nov
// 2026), so their logs are kept here, for as long as the window lives.
//
// **Its words as they're written** (12 Nov 2026, `saying`): the message an
// agent is writing now, so far (`yap-agent-saying`, agents/runner.rs: many
// times a second, to the page only, never a line of its log). Let go once
// the message is whole (its `said` line, its final words), it moves on to a
// step, or it's over (core/agentNow.js `sayingOver`).
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { sayingOver } from '../core/agentNow.js';

export const agentLogs = $state({
  /** Agent id → its events. Absent until read. */
  by: {},
  /** Agent id → `{turn, text}`: the message it's writing now, so far. */
  saying: {},
});

let hearingWords = false;
/** Follow what agents write as they write it (once per window: asking again does nothing). */
export function watchSaying() {
  if (hearingWords) return;
  hearingWords = true;
  listen('yap-agent-saying', (e) => {
    const p = e.payload;
    if (p?.id && typeof p.text === 'string') agentLogs.saying[p.id] = { turn: Number(p.turn) || 0, text: p.text };
  }).catch(() => {});
  listen('yap-agent', (e) => {
    const ev = e.payload;
    if (ev?.id && agentLogs.saying[ev.id] && sayingOver(ev)) delete agentLogs.saying[ev.id];
  }).catch(() => {});
}

/** id → `{loading, heard}`: events heard while its log is being read wait in `heard`. */
const watched = new Map();
let listening = false;

/** What a turn has used so far (agents/usage.rs) is its note's, never a line of the log. */
const usageOnly = (ev) => !!ev.usage && !ev.step && !ev.now && !ev.text && !ev.todos && !ev.said && typeof ev.cost !== 'number';

function onAgent(ev) {
  const w = ev?.id ? watched.get(ev.id) : null;
  if (!w || usageOnly(ev)) return;
  if (w.loading) w.heard.push(ev);
  else agentLogs.by[ev.id] = [...(agentLogs.by[ev.id] || []), ev];
}

/** Keep agent `id`'s log from now on (once: asking again does nothing). */
export function watchAgentLog(id) {
  if (!id || watched.has(id)) return;
  const w = { loading: true, heard: [] };
  watched.set(id, w);
  if (!listening) {
    listening = true;
    listen('yap-agent', (e) => onAgent(e.payload)).catch(() => {});
  }
  invoke('agent_get', { id })
    .then((r) => {
      // (An event heard while it was read may be in what was read too.)
      const saved = r?.log || [];
      const tail = new Set(saved.slice(-50).map((e) => JSON.stringify(e)));
      agentLogs.by[id] = [...saved, ...w.heard.filter((e) => !tail.has(JSON.stringify(e)))];
    })
    .catch(() => {
      agentLogs.by[id] = [...w.heard];
    })
    .finally(() => {
      w.loading = false;
      w.heard = [];
    });
}
