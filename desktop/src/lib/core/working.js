// **What's at work, in a few words** (the Yap bar's line, bar/Overlay.svelte,
// and core/agents.js `runningLine`): agents sent out, and Code chats at work
// on the person's own words (agents/direct.rs) — the chat's own Claude, as
// Claude Code's own app has its session, never "an agent" (14 Nov 2026).
// Its own small module: the bar's chunk stays small. Pure.

/** "1 agent working", "3 agents working", "Claude is working", "Claude and 1 agent working"; '' for nothing. */
export function workingLine(agents = 0, chats = 0) {
  const n = Math.max(0, Number(agents) || 0);
  const c = Math.max(0, Number(chats) || 0);
  const sent = n === 1 ? '1 agent' : `${n} agents`;
  if (!c) return n ? `${sent} working` : '';
  if (n) return `Claude and ${sent} working`;
  return c === 1 ? 'Claude is working' : `Claude is working in ${c} chats`;
}
