// **Code chats in the Chat view** (desktop/src-tauri/src/agents/code_chat.rs,
// core/codeChats.js). With Yap Code on, Chat has two sides, as the Claude app's
// sidebar switch has them: **Chat** (plain conversations, exactly as with
// Yap Code off: no Mode chip, nothing agent-like) and **Code** (conversations
// attached to one of the person's workspaces — the workspaces — grouped
// by workspace, with the running agents on top). The switch is kept between
// visits (this window's storage); the side follows the conversation shown
// (one opened from the running agents, the Voice Assistant's, a plain chat
// that just started an agent and became a Code chat).
//
// - **A new code chat** ("New code chat", the workspace picker) waits for
//   its first question with its workspace (`pending`), which goes with it
//   (`chat_send`'s `codeFolder`): Rust attaches the conversation it makes
//   before the prompt is built. Asked out loud instead (the Voice
//   Assistant), the same: this view tells Rust the workspace it shows
//   (`chat_view_state`'s `codeFolder`), and the question's
//   `yap-assistant-turn` says it was attached (its `codeFolder`). Switching
//   to Code with no code chat open starts one on the most recently used
//   workspace. Empty, it greets you as the Code side (CodeEmpty.svelte).
// - **The workspace chip** in a Code chat's composer switches its workspace
//   (`code_chat_workspace_set`) or makes it a plain chat again.
// - **Where it's kept**: Rust's `agents.json` (`code_chats_list`), never
//   chats.json; every window hears a change (`yap-code-chat`).
// - **The look** (codeChat.css): the accent edge and its label on the box,
//   and the slow light along it while an agent from this chat runs
//   ("Animate Code chats", `agentsAnimate`; never with reduced motion).
//
// Made by ChatView, beside the chat's own state (chatState.svelte.js), which
// it gives the list's grouping (`groupBy`) and a new chat's workspace.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { untrack } from 'svelte';
import { agents } from '../../stores/agents.svelte.js';
import { isLive } from '../../core/agents.js';
import { groupByWorkspace, plainOnly, sideOf, samePath } from '../../core/codeChats.js';

const SIDE_KEY = 'yap.chat.side';
const FOLDED_KEY = 'yap.chat.codeGroupsFolded';

function read(key, fallback) {
  try {
    const v = localStorage.getItem(key);
    return v == null ? fallback : JSON.parse(v);
  } catch {
    return fallback;
  }
}
function write(key, value) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    /* a per-window convenience: fine without it */
  }
}

/**
 * `chat`: the view's state (chatState.svelte.js). `opensettings(section)`:
 * Settings (Manage workspaces… opens Settings → Yap Code).
 */
export function createCodeChats(chat, { opensettings = () => null } = {}) {
  let enabled = $state(false);
  let animate = $state(true);
  /** How agents pay (`agentsPay`): "plan" needs Claude Code signed in, "key" doesn't. */
  let pay = $state('plan');
  let loaded = $state(false);
  /** Conversation id → its workspace (`{name, path, cap?}`). */
  let chats = $state({});
  /** The workspaces, most recently used first (`{name, path, cap?, addedTs, usedTs}`). */
  let workspaces = $state([]);
  let side = $state(read(SIDE_KEY, 'chat') === 'code' ? 'code' : 'chat');
  /** A new chat's workspace, until its first question makes its conversation. */
  let pending = $state(null);
  /** Groups folded on the Code side (path → true). */
  let folded = $state(read(FOLDED_KEY, {}) || {});
  // The workspace a new chat's question went with (Rust attaches its
  // conversation; this window marks it at once).
  let sentWith = null;
  // The conversation last shown on each side (the switch goes back to it).
  const lastOn = { chat: null, code: null };

  const of = (id) => (id == null ? null : (chats[id] ?? null));
  /** The workspace the conversation shown is in (a new chat's: its pick). */
  const current = $derived(enabled ? (chat.activeId != null ? of(chat.activeId) : pending) : null);
  /** The Code side is showing. */
  const onCode = $derived(enabled && side === 'code');
  /** An agent from this chat is running. */
  const running = $derived(chat.activeId != null && agents.list.some((j) => j.conversation === chat.activeId && isLive(j.status)));

  async function loadConfig() {
    try {
      const cfg = await invoke('get_config');
      enabled = !!cfg?.agentsEnabled;
      animate = cfg?.agentsAnimate !== false;
      pay = cfg?.agentsPay === 'key' ? 'key' : 'plan';
    } catch {
      /* keep */
    }
  }

  /** Every Code chat and the workspaces, from Rust. */
  async function refresh() {
    try {
      const r = await invoke('code_chats_list');
      chats = r?.chats ?? {};
      workspaces = r?.workspaces ?? [];
      loaded = true;
      // A new code chat's pick that's no longer a workspace (removed in
      // Settings → Yap Code) lets it go: the most recent one instead, or none
      // ("Choose a workspace"). One still there takes its latest name and cap.
      if (pending) {
        const still = workspaces.find((w) => samePath(w.path, pending.path));
        pending = still ?? (side === 'code' && chat.activeId == null ? (workspaces[0] ?? null) : null);
      }
    } catch {
      /* keep the last */
    }
  }

  function setSideNow(next) {
    side = next;
    write(SIDE_KEY, next);
  }

  // The list shows one side (with Yap Code off: everything, as ever).
  chat.groupBy = (conversations, byDate) => {
    if (!enabled) return byDate(conversations);
    if (side === 'chat') return byDate(plainOnly(conversations, chats));
    return groupByWorkspace(conversations, chats, folded);
  };
  // A new chat's first question carries its workspace.
  chat.newChatFolder = () => {
    if (!enabled || !pending) return null;
    sentWith = pending;
    return pending.path;
  };
  // …and Rust knows it, for a question asked out loud (`chat_view_state`).
  chat.pendingFolder = () => (enabled && pending ? pending.path : null);
  $effect(() => {
    void pending;
    void enabled;
    untrack(() => {
      if (chat.activeId == null) chat.reportView();
    });
  });

  // The conversation shown changed: a new code chat's conversation marked at
  // once (Rust attached it as it was made), a new chat on the Code side
  // given a workspace, and each side's last one kept. (Before the page
  // draws: the box never flickers plain.)
  let lastId = null;
  $effect.pre(() => {
    const id = chat.activeId ?? null;
    untrack(() => {
      const was = lastId;
      if (id === was) return;
      lastId = id;
      if (was === null && id !== null && sentWith && chat.sending) {
        chats = { ...chats, [id]: sentWith };
        pending = null;
        sentWith = null;
        // (Rust's word on it — `yap-code-chat` — settles it; a moment later regardless.)
        setTimeout(refresh, 1500);
      } else if (was === null && id !== null && pending && samePath(of(id)?.path, pending.path)) {
        // A question asked out loud made this new code chat's conversation
        // (marked as it was asked: `yap-assistant-turn`).
        pending = null;
      } else if (id === null) {
        sentWith = null;
        if (enabled && side === 'code' && !pending) pending = of(was) ?? workspaces[0] ?? null;
      }
      if (id !== null) lastOn[sideOf(id, chats)] = id;
    });
  });

  // A question over (or never sent): its workspace no longer waits for a conversation.
  $effect(() => {
    if (!chat.sending) untrack(() => (sentWith = null));
  });

  // The side follows the conversation shown (once Rust's list is in).
  $effect(() => {
    const id = chat.activeId ?? null;
    if (!enabled || !loaded || id === null) return;
    const want = sideOf(id, chats);
    untrack(() => {
      if (side !== want) setSideNow(want);
      if (want === 'chat') pending = null;
    });
  });

  // Yap Code on, the list's side shown: a new chat on the Code side gets a
  // workspace once they're known.
  $effect(() => {
    if (!enabled || !loaded || side !== 'code') return;
    untrack(() => {
      if (chat.activeId == null && !pending && workspaces.length) pending = workspaces[0];
    });
  });

  $effect(() => {
    loadConfig();
    refresh();
    const uns = [
      listen('yap-config-saved', loadConfig),
      listen('yap-code-chat', () => refresh()),
      // An agent started: the workspaces' order changes.
      listen('yap-agent', (e) => {
        if (e.payload?.status === 'queued') refresh();
      }),
      // A question asked out loud made a new code chat's conversation: a
      // Code chat at once, before Chat shows it (the box never flickers plain).
      listen('yap-assistant-turn', (e) => {
        const p = e.payload;
        if (p?.phase !== 'asked' || p.conversationId == null || !p.codeFolder?.path) return;
        chats = { ...chats, [p.conversationId]: p.codeFolder };
        setTimeout(refresh, 1500);
      }),
    ];
    return () => uns.forEach((u) => u.then((f) => f()).catch(() => {}));
  });

  /** **The switch**: Chat or Code — the side's last conversation, else a new one. */
  function setSide(next) {
    if (!enabled || next === side) return;
    setSideNow(next);
    const back = lastOn[next];
    const there = back != null && sideOf(back, chats) === next && chat.conversations.some((c) => c.id === back);
    if (there) {
      chat.selectConversation(back);
      return;
    }
    pending = next === 'code' ? (workspaces[0] ?? null) : null;
    chat.newChat();
  }

  /** The Chat side for a plain chat about to be made (a question from another view). */
  function toChat() {
    pending = null;
    if (enabled && side !== 'chat') setSideNow('chat');
  }

  /** **New code chat** in `ws`. */
  function newCodeChat(ws) {
    if (!ws) return;
    setSideNow('code');
    pending = ws;
    chat.newChat();
  }

  /** The workspace chip's pick: this chat's workspace (a new chat's waits for its question). */
  async function setWorkspace(ws) {
    if (!ws) return;
    const id = chat.activeId;
    if (id == null) {
      pending = ws;
      return;
    }
    if (samePath(of(id)?.path, ws.path)) return;
    chats = { ...chats, [id]: ws };
    try {
      await invoke('code_chat_workspace_set', { conversation: id, folder: ws.path });
    } catch {
      refresh();
    }
  }

  /** "Plain chat": this chat leaves Code (to the Chat side). */
  async function makePlain() {
    const id = chat.activeId;
    if (id == null) {
      pending = null;
      setSideNow('chat');
      return;
    }
    const { [id]: _gone, ...rest } = chats;
    chats = rest;
    try {
      await invoke('code_chat_workspace_set', { conversation: id, folder: null });
    } catch {
      refresh();
    }
  }

  /** Add workspace…: Windows' folder picker (Rust), the folder picked added. Its workspace, or null. */
  async function addWorkspace() {
    const f = await invoke('workspaces_pick');
    await refresh();
    if (!f) return null;
    return workspaces.find((w) => samePath(w.path, f.path)) ?? f;
  }

  function fold(path) {
    folded = { ...folded, [path]: !folded[path] };
    if (!folded[path]) delete folded[path];
    write(FOLDED_KEY, folded);
  }

  return {
    get enabled() { return enabled; },
    get loaded() { return loaded; },
    get pay() { return pay; },
    get side() { return side; },
    get onCode() { return onCode; },
    get current() { return current; },
    get pending() { return pending; },
    get workspaces() { return workspaces; },
    get chats() { return chats; },
    get running() { return running; },
    /** The slow light along the box: an agent running here, animation on. */
    get moving() { return !!current && running && animate; },
    get animate() { return animate; },
    of,
    refresh,
    setSide,
    toChat,
    newCodeChat,
    setWorkspace,
    makePlain,
    addWorkspace,
    fold,
    manage: () => opensettings()?.('yap-code'),
  };
}
