// **Agents' logs for the thread** (views/chat/AgentWork.svelte): each watched
// agent's `yap-agent` events, oldest first — read once (`agent_get`, as its
// tab reads them) and kept in step with the app-wide event from then on. The
// agents store (agents.svelte.js) keeps each agent's record and the line it's
// on, never its log; the Agent tab keeps the open agent's for itself. A Code
// chat's thread shows what its agents said and did as they work (11 Nov
// 2026), so their logs are kept here, for as long as the window lives.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export const agentLogs = $state({
  /** Agent id → its events. Absent until read. */
  by: {},
});

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
